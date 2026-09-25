//! `select`: an if/else (`OpSelectionMerge` + `OpBranchConditional`) whose arms are single
//! blocks of speculatable instructions (no stores, image ops, derivatives, kills, calls, loops;
//! see [`super::is_speculatable`]) that both branch to the merge block, where the merge block's
//! phis are the only join points, becomes straight-line code: the arms' instructions move into
//! the header (true arm first), each phi becomes an `OpSelect` with the same result id, and the
//! branch is dropped. One arm may be empty (the branch goes straight to the merge block, as
//! glslang emits for `a || b` and `a && b`). Class `exact`. `--only-op` matches the header
//! label, the merge label or any phi id.

use super::cfg::Cfg;
use super::{id_op, is_speculatable, remove_metadata, vector_shape, Class, EditOp, Opts};
use crate::lift::{Lifted, Type};
use rspirv::dr::{Instruction, Operand};
use spirv::Op;
use std::collections::HashSet;

pub fn run(l: &mut Lifted, ops: &mut Vec<EditOp>, opts: &Opts) -> usize {
    let mut total = 0;
    // One transformation per round: block indices change after each.
    while let Some(()) = one(l, ops, opts) {
        total += 1;
    }
    total
}

struct Site {
    fi: usize,
    h: usize,
    t: usize,
    f: usize,
    m: usize,
    cond: u32,
}

fn find(l: &Lifted, opts: &Opts) -> Option<Site> {
    for fi in 0..l.module.functions.len() {
        let cfg = Cfg::build(l, fi);
        let func = &l.module.functions[fi];
        let info = &l.functions[fi];
        for (h, hb) in func.blocks.iter().enumerate() {
            let Some(st) = info.headers.get(&cfg.labels[h]) else { continue };
            if st.continue_target.is_some() {
                continue;
            }
            let Some(term) = hb.instructions.last() else { continue };
            if term.class.opcode != Op::BranchConditional {
                continue;
            }
            let (Some(cond), Some(tl), Some(fl)) = (id_op(term, 0), id_op(term, 1), id_op(term, 2)) else { continue };
            let (Some(&t), Some(&f), Some(&m)) = (cfg.index.get(&tl), cfg.index.get(&fl), cfg.index.get(&st.merge)) else { continue };
            if t == f {
                continue;
            }
            // The condition must be a scalar bool (vector conditions cannot select composites
            // before SPIR-V 1.4).
            let cty = l.result_types.get(&cond).copied().unwrap_or(0);
            if !matches!(l.types.get(&cty), Some(Type::Bool)) {
                continue;
            }
            // Merge block predecessors must be exactly the two arms (or the header for an
            // empty arm).
            let mut expected: Vec<usize> = [t, f].iter().map(|&a| if a == m { h } else { a }).collect();
            expected.sort_unstable();
            let mut preds = cfg.pred[m].clone();
            preds.sort_unstable();
            if preds != expected {
                continue;
            }
            let mut ok = true;
            for arm in [t, f] {
                if arm == m {
                    continue;
                }
                if arm == h || cfg.pred[arm] != vec![h] {
                    ok = false;
                    break;
                }
                let ab = &func.blocks[arm];
                let Some(at) = ab.instructions.last() else {
                    ok = false;
                    break;
                };
                if at.class.opcode != Op::Branch || id_op(at, 0) != Some(st.merge) {
                    ok = false;
                    break;
                }
                let body = &ab.instructions[..ab.instructions.len() - 1];
                if body.len() > opts.max_select_arm || !body.iter().all(|i| is_speculatable(l, i)) {
                    ok = false;
                    break;
                }
            }
            if !ok {
                continue;
            }
            // Merge block: leading phis with exactly two incoming, selectable types.
            let mb = &func.blocks[m];
            let phis: Vec<&Instruction> = mb.instructions.iter().take_while(|i| i.class.opcode == Op::Phi).collect();
            if mb.instructions.iter().skip(phis.len()).any(|i| i.class.opcode == Op::Phi) {
                continue;
            }
            if !phis.iter().all(|p| {
                p.operands.len() == 4
                    && p.result_type.map_or(false, |ty| {
                        vector_shape(l, ty).map_or(false, |(e, _)| matches!(l.types.get(&e), Some(Type::Bool | Type::Int { .. } | Type::Float { .. })))
                    })
            }) {
                continue;
            }
            let allowed = opts.only_op.map_or(true, |o| {
                o == cfg.labels[h] || o == cfg.labels[m] || phis.iter().any(|p| p.result_id == Some(o))
            });
            if !allowed {
                continue;
            }
            return Some(Site { fi, h, t, f, m, cond });
        }
    }
    None
}

fn one(l: &mut Lifted, ops: &mut Vec<EditOp>, opts: &Opts) -> Option<()> {
    let s = find(l, opts)?;
    let func = &mut l.module.functions[s.fi];
    let label = |b: usize, func: &rspirv::dr::Function| func.blocks[b].label.as_ref().unwrap().result_id.unwrap();
    let (hl, tl, fl, ml) = (label(s.h, func), label(s.t, func), label(s.f, func), label(s.m, func));
    // Arm bodies (without terminators).
    let take_body = |b: usize, func: &rspirv::dr::Function| -> Vec<Instruction> {
        if b == s.m {
            return vec![];
        }
        let ins = &func.blocks[b].instructions;
        ins[..ins.len() - 1].to_vec()
    };
    let tb = take_body(s.t, func);
    let fb = take_body(s.f, func);
    let n_arm = tb.len() + fb.len();
    // Header: drop OpSelectionMerge and the conditional branch; append the arms and a branch.
    {
        let hb = &mut func.blocks[s.h];
        hb.instructions.pop();
        hb.instructions.retain(|i| i.class.opcode != Op::SelectionMerge);
        hb.instructions.extend(tb);
        hb.instructions.extend(fb);
        hb.instructions.push(Instruction::new(Op::Branch, None, None, vec![Operand::IdRef(ml)]));
    }
    // Merge block phis -> OpSelect.
    let mut phi_ids = Vec::new();
    {
        let mb = &mut func.blocks[s.m];
        for inst in mb.instructions.iter_mut() {
            if inst.class.opcode != Op::Phi {
                break;
            }
            let (v0, l0, v1, l1) = (id_op(inst, 0).unwrap(), id_op(inst, 1).unwrap(), id_op(inst, 2).unwrap(), id_op(inst, 3).unwrap());
            let from_true = if s.t == s.m { hl } else { tl };
            let (vt, vf) = if l0 == from_true { (v0, v1) } else if l1 == from_true { (v1, v0) } else { (v0, v1) };
            phi_ids.push(inst.result_id.unwrap());
            *inst = Instruction::new(
                Op::Select,
                inst.result_type,
                inst.result_id,
                vec![Operand::IdRef(s.cond), Operand::IdRef(vt), Operand::IdRef(vf)],
            );
        }
    }
    // Remove the arm blocks.
    let mut gone: HashSet<u32> = HashSet::new();
    let mut drop: Vec<usize> = Vec::new();
    for arm in [s.t, s.f] {
        if arm != s.m {
            gone.insert(label(arm, func));
            drop.push(arm);
        }
    }
    drop.sort_unstable();
    for b in drop.into_iter().rev() {
        func.blocks.remove(b);
    }
    remove_metadata(l, &gone);
    ops.push(EditOp {
        pass: "select",
        class: Class::Exact,
        target: hl,
        replaced_by: Some(ml),
        detail: format!(
            "if/else %{hl} (cond %{}) arms %{tl}/%{fl} ({n_arm} instructions) -> {} OpSelect ({}) in merge %{ml}",
            s.cond,
            phi_ids.len(),
            phi_ids.iter().map(|p| format!("%{p}")).collect::<Vec<_>>().join(",")
        ),
    });
    l.reanalyze().expect("reanalyze after select");
    Some(())
}
