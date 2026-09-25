//! `unroll`: full unrolling of counted loops. A loop is recognised from `OpLoopMerge` when
//!
//! * its only exit is the conditional branch of the check block (the header itself, or the
//!   header's single successor as glslang emits `for`/`while`), whose condition is an integer
//!   comparison of the induction value against a constant;
//! * the induction value is either an `OpPhi` in the header with a constant initial value and
//!   a back-edge value `phi +/- constant` (SSA form), or, as glslang `-V` emits, an `OpLoad` of
//!   a Function-storage variable that is stored exactly twice: a constant in the block before
//!   the header and `load +/- constant` in the continue block, and used only by loads/stores;
//! * the trip count, simulated in 32-bit integer arithmetic, is `<= max-unroll`;
//! * the loop contains no other loop (inner loops are unrolled first, then the outer one on the
//!   next round), its blocks are contiguous in module order, and the continue block has a
//!   single predecessor (no `continue` from inside a nested `if`: without the loop that branch
//!   would leave the selection construct, which structured control flow forbids).
//!
//! The check block(s) are cloned `N+1` times and the body (including the continue block) `N`
//! times, with fresh ids; the induction phi / the loads of the induction variable are replaced
//! by the iteration's constant; the loop structure (`OpLoopMerge`, the conditional exit) is
//! dropped. The header keeps its label so that enclosing structures stay valid. Other header
//! phis are threaded through the iterations. Class `exact`. Decorations on cloned results are
//! copied; `OpName`s of removed ids are dropped. `--only-op` matches the header label.

use super::cfg::Cfg;
use super::consts::{self, CV};
use super::{copy_decorations, def_inst, fresh_id, id_op, real_uses, remove_metadata, Class, EditOp, Opts};
use crate::lift::{Lifted, Site, Type};
use rspirv::dr::{Block, Instruction, Operand};
use spirv::{Op, StorageClass};
use std::collections::{HashMap, HashSet};

pub fn run(l: &mut Lifted, ops: &mut Vec<EditOp>, opts: &Opts) -> usize {
    let mut total = 0;
    while one(l, ops, opts).is_some() {
        total += 1;
    }
    total
}

#[derive(Clone, Debug)]
enum Induction {
    Phi { phi: u32 },
    Var { var: u32, load_after: HashSet<u32> },
}

struct Found {
    fi: usize,
    h: usize,
    x: usize,
    m: usize,
    /// All loop blocks in module order (`h..h+len`).
    blocks: Vec<usize>,
    b0: u32,
    exit_label: u32,
    /// The exit condition, when its only use is the conditional branch (not cloned).
    dead_cond: Option<u32>,
    induction: Induction,
    int_ty: u32,
    init: i32,
    step: i32,
    trip: u32,
    desc: String,
}

fn cmp(op: Op, a: u32, b: u32) -> Option<bool> {
    Some(match op {
        Op::IEqual => a == b,
        Op::INotEqual => a != b,
        Op::SLessThan => (a as i32) < (b as i32),
        Op::SLessThanEqual => (a as i32) <= (b as i32),
        Op::SGreaterThan => (a as i32) > (b as i32),
        Op::SGreaterThanEqual => (a as i32) >= (b as i32),
        Op::ULessThan => a < b,
        Op::ULessThanEqual => a <= b,
        Op::UGreaterThan => a > b,
        Op::UGreaterThanEqual => a >= b,
        _ => return None,
    })
}

fn int_const(l: &Lifted, id: u32) -> Option<i32> {
    match consts::read(l, id)? {
        CV::I(v) => Some(v as i32),
        _ => None,
    }
}

/// `next = iv +/- const` (or `const + iv`): the signed step.
fn step_of(l: &Lifted, next: u32, iv: u32) -> Option<i32> {
    let inst = def_inst(l, next)?;
    let (a, b) = (id_op(inst, 0)?, id_op(inst, 1)?);
    match inst.class.opcode {
        Op::IAdd => {
            if a == iv {
                int_const(l, b)
            } else if b == iv {
                int_const(l, a)
            } else {
                None
            }
        }
        Op::ISub if a == iv => int_const(l, b).map(|s| s.wrapping_neg()),
        _ => None,
    }
}

fn find(l: &Lifted, opts: &Opts) -> Option<Found> {
    for fi in 0..l.module.functions.len() {
        let cfg = Cfg::build(l, fi);
        let func = &l.module.functions[fi];
        let info = &l.functions[fi];
        for h in 0..func.blocks.len() {
            let hl = cfg.labels[h];
            let Some(st) = info.headers.get(&hl) else { continue };
            let Some(cl) = st.continue_target else { continue };
            if !opts.allows(hl) {
                continue;
            }
            let (Some(&m), Some(&c)) = (cfg.index.get(&st.merge), cfg.index.get(&cl)) else { continue };
            // Loop blocks: reachable from h without passing through m.
            let mut lset: HashSet<usize> = HashSet::new();
            let mut stack = vec![h];
            while let Some(b) = stack.pop() {
                if b == m || !lset.insert(b) {
                    continue;
                }
                for &s in &cfg.succ[b] {
                    stack.push(s);
                }
            }
            if !lset.contains(&c) {
                continue;
            }
            // No inner loops; contiguous in module order.
            if lset.iter().any(|&b| b != h && info.headers.get(&cfg.labels[b]).map_or(false, |s| s.continue_target.is_some())) {
                continue;
            }
            let mut blocks: Vec<usize> = lset.iter().copied().collect();
            blocks.sort_unstable();
            if blocks.first() != Some(&h) || blocks.last() != Some(&(h + blocks.len() - 1)) {
                continue;
            }
            // Single outside predecessor p and the back edge from c.
            let outside: Vec<usize> = cfg.pred[h].iter().copied().filter(|b| !lset.contains(b)).collect();
            let [p] = outside[..] else { continue };
            if cfg.pred[h].len() != 2 || !cfg.pred[h].contains(&c) {
                continue;
            }
            // The continue block must have a single predecessor: a `continue` from inside a
            // nested selection would become a branch out of that construct once the loop is
            // gone, which structured control flow forbids.
            if cfg.pred[c].len() != 1 {
                continue;
            }
            let cterm = func.blocks[c].instructions.last()?;
            if cterm.class.opcode != Op::Branch || id_op(cterm, 0) != Some(hl) {
                continue;
            }
            // Check block x.
            let hterm = func.blocks[h].instructions.last()?;
            let x = match hterm.class.opcode {
                Op::BranchConditional => h,
                Op::Branch => {
                    let Some(&x) = cfg.index.get(&id_op(hterm, 0)?) else { continue };
                    if !lset.contains(&x) || cfg.pred[x] != vec![h] || x == c {
                        continue;
                    }
                    if func.blocks[x].instructions.last()?.class.opcode != Op::BranchConditional {
                        continue;
                    }
                    x
                }
                _ => continue,
            };
            if cfg.pred[m] != vec![x] {
                continue;
            }
            let xterm = func.blocks[x].instructions.last()?;
            let (Some(cond), Some(tt), Some(ff)) = (id_op(xterm, 0), id_op(xterm, 1), id_op(xterm, 2)) else { continue };
            let (b0, cont_when_true) = if tt == st.merge && ff != st.merge {
                (ff, false)
            } else if ff == st.merge && tt != st.merge {
                (tt, true)
            } else {
                continue;
            };
            if !cfg.index.get(&b0).map_or(false, |b| lset.contains(b)) {
                continue;
            }
            // Condition: integer compare of the induction value against a constant, defined
            // in h or x.
            let Some(Site::Inst(_, cb, _)) = l.defs.get(&cond).copied() else { continue };
            if cb != h && cb != x {
                continue;
            }
            let cinst = def_inst(l, cond)?;
            let cop = cinst.class.opcode;
            if cmp(cop, 0, 0).is_none() {
                continue;
            }
            let (Some(ca), Some(cb_)) = (id_op(cinst, 0), id_op(cinst, 1)) else { continue };
            let (iv, k, iv_left) = if let Some(k) = int_const(l, cb_) {
                (ca, k, true)
            } else if let Some(k) = int_const(l, ca) {
                (cb_, k, false)
            } else {
                continue;
            };
            let Some(ivinst) = def_inst(l, iv) else { continue };
            let int_ty = ivinst.result_type.unwrap_or(0);
            if !matches!(l.types.get(&int_ty), Some(Type::Int { width: 32, .. })) {
                continue;
            }
            let (induction, init, step, desc) = match ivinst.class.opcode {
                Op::Phi => {
                    let Some(Site::Inst(_, pb, _)) = l.defs.get(&iv).copied() else { continue };
                    if pb != h || ivinst.operands.len() != 4 {
                        continue;
                    }
                    let (v0, l0, v1, l1) = (id_op(ivinst, 0)?, id_op(ivinst, 1)?, id_op(ivinst, 2)?, id_op(ivinst, 3)?);
                    let pl = cfg.labels[p];
                    let (init_id, next) = if l0 == pl && l1 == cl {
                        (v0, v1)
                    } else if l1 == pl && l0 == cl {
                        (v1, v0)
                    } else {
                        continue;
                    };
                    let Some(init) = int_const(l, init_id) else { continue };
                    let Some(step) = step_of(l, next, iv) else { continue };
                    match l.defs.get(&next) {
                        Some(Site::Inst(_, nb, _)) if lset.contains(nb) => {}
                        _ => continue,
                    }
                    (Induction::Phi { phi: iv }, init, step, format!("phi %{iv}"))
                }
                Op::Load => {
                    let Some(Site::Inst(_, lb, _)) = l.defs.get(&iv).copied() else { continue };
                    if lb != h && lb != x {
                        continue;
                    }
                    let Some(var) = id_op(ivinst, 0) else { continue };
                    let Some(vinst) = def_inst(l, var) else { continue };
                    if vinst.class.opcode != Op::Variable
                        || !matches!(vinst.operands.first(), Some(Operand::StorageClass(StorageClass::Function)))
                        || !matches!(l.defs.get(&var), Some(Site::Inst(vf, _, _)) if *vf == fi)
                    {
                        continue;
                    }
                    // Uses: loads and stores only; exactly two stores: a constant in p and
                    // `load +/- const` in a loop block S that dominates the continue block
                    // (executed exactly once per iteration). Loads inside the loop are
                    // classified "before" the store (they read the iteration's value) or
                    // "after" (they read the next one) by dominance; anything else rejects.
                    let mut init: Option<i32> = None;
                    let mut step: Option<i32> = None;
                    let mut n_stores = 0;
                    let mut ok = true;
                    let mut store_at: Option<(usize, usize)> = None;
                    let mut loads: Vec<(usize, usize, u32)> = Vec::new();
                    for site in real_uses(l, var) {
                        let Site::Inst(uf, ub, ui) = *site else {
                            ok = false;
                            break;
                        };
                        let u = &l.module.functions[uf].blocks[ub].instructions[ui];
                        match u.class.opcode {
                            Op::Load if id_op(u, 0) == Some(var) => {
                                if uf == fi && lset.contains(&ub) {
                                    loads.push((ub, ui, u.result_id.unwrap()));
                                }
                            }
                            Op::Store if id_op(u, 0) == Some(var) && id_op(u, 1) != Some(var) => {
                                n_stores += 1;
                                let val = id_op(u, 1).unwrap();
                                if ub == p && uf == fi {
                                    init = int_const(l, val);
                                } else if uf == fi && lset.contains(&ub) && ub != h && ub != x && cfg.dominates(ub, c) {
                                    let Some(vi) = def_inst(l, val) else {
                                        ok = false;
                                        break;
                                    };
                                    let (Some(a), Some(b)) = (id_op(vi, 0), id_op(vi, 1)) else {
                                        ok = false;
                                        break;
                                    };
                                    let load_id = [a, b].into_iter().find(|&o| {
                                        def_inst(l, o).map_or(false, |d| d.class.opcode == Op::Load && id_op(d, 0) == Some(var))
                                    });
                                    let Some(load_id) = load_id else {
                                        ok = false;
                                        break;
                                    };
                                    match (l.defs.get(&load_id), l.defs.get(&val)) {
                                        (Some(Site::Inst(_, lb, li)), Some(Site::Inst(_, nb, _))) if *lb == ub && *li < ui && *nb == ub => {}
                                        _ => {
                                            ok = false;
                                            break;
                                        }
                                    }
                                    step = step_of(l, val, load_id);
                                    store_at = Some((ub, ui));
                                } else {
                                    ok = false;
                                    break;
                                }
                            }
                            _ => {
                                ok = false;
                                break;
                            }
                        }
                    }
                    let (Some(init), Some(step), Some((sb, si))) = (init, step, store_at) else { continue };
                    if !ok || n_stores != 2 {
                        continue;
                    }
                    // Blocks reachable from the store block within the iteration (not through
                    // the header): a load there reads the next value only if the store block
                    // dominates it; a load elsewhere reads the iteration's value.
                    let mut reach: HashSet<usize> = HashSet::new();
                    let mut stack: Vec<usize> = cfg.succ[sb].clone();
                    while let Some(b) = stack.pop() {
                        if b == h || b == m || !lset.contains(&b) || !reach.insert(b) {
                            continue;
                        }
                        stack.extend(cfg.succ[b].iter().copied());
                    }
                    let mut load_after: HashSet<u32> = HashSet::new();
                    for (lb, li, lid) in loads {
                        if lb == sb {
                            if li > si {
                                load_after.insert(lid);
                            }
                        } else if !reach.contains(&lb) {
                        } else if cfg.dominates(sb, lb) {
                            load_after.insert(lid);
                        } else {
                            ok = false;
                        }
                    }
                    if !ok {
                        continue;
                    }
                    let name = l.name(var).map(|n| format!(" \"{n}\"")).unwrap_or_default();
                    (Induction::Var { var, load_after }, init, step, format!("variable %{var}{name}"))
                }
                _ => continue,
            };
            // Trip count.
            let mut i = init;
            let mut trip = 0u32;
            let mut fits = true;
            loop {
                let (a, b) = if iv_left { (i as u32, k as u32) } else { (k as u32, i as u32) };
                let go = cmp(cop, a, b).unwrap() == cont_when_true;
                if !go {
                    break;
                }
                trip += 1;
                if trip > opts.max_unroll {
                    fits = false;
                    break;
                }
                i = i.wrapping_add(step);
            }
            if !fits {
                continue;
            }
            let cname = format!("{:?}", cop).trim_start_matches("Op").to_string();
            return Some(Found {
                fi,
                h,
                x,
                m,
                blocks,
                b0,
                exit_label: cfg.labels[x],
                dead_cond: if real_uses(l, cond).count() == 1 { Some(cond) } else { None },
                induction,
                int_ty,
                init,
                step,
                trip,
                desc: format!(
                    "loop %{hl}: {desc} = {init}; exit when !({}{cname} {}); step {step:+}; {trip} iterations",
                    if iv_left { "i ".to_string() } else { format!("{k} ") },
                    if iv_left { k.to_string() } else { "i".into() }
                ),
            });
        }
    }
    None
}

fn one(l: &mut Lifted, ops: &mut Vec<EditOp>, opts: &Opts) -> Option<()> {
    let f = find(l, opts)?;
    let n = f.trip as usize;
    let fi = f.fi;
    let hl = l.module.functions[fi].blocks[f.h].label.as_ref()?.result_id?;
    let ml = l.module.functions[fi].blocks[f.m].label.as_ref()?.result_id?;
    let check: Vec<usize> = if f.x == f.h { vec![f.h] } else { vec![f.h, f.x] };
    let orig: Vec<Block> = f.blocks.iter().map(|&b| l.module.functions[fi].blocks[b].clone()).collect();
    let orig_of = |b: usize| &orig[b - f.h];
    let label_of = |b: &Block| b.label.as_ref().unwrap().result_id.unwrap();
    // Loop-defined ids.
    let mut loop_ids: HashSet<u32> = HashSet::new();
    for b in &orig {
        loop_ids.insert(label_of(b));
        for i in &b.instructions {
            if let Some(r) = i.result_id {
                loop_ids.insert(r);
            }
        }
    }
    // Header phis.
    let hphis: Vec<Instruction> = orig_of(f.h).instructions.iter().filter(|i| i.class.opcode == Op::Phi).cloned().collect();
    let pl = {
        // The outside predecessor label: the phi operand label that is not the continue label.
        let cl = l.functions[fi].headers[&hl].continue_target.unwrap();
        hphis.first().and_then(|p| [id_op(p, 1), id_op(p, 3)].into_iter().flatten().find(|&x| x != cl))
    };
    // Per-iteration constants.
    let mut consts_k: Vec<u32> = Vec::with_capacity(n + 1);
    let mut i = f.init;
    for _ in 0..=n {
        consts_k.push(consts::intern(l, f.int_ty, &CV::I(i as u32))?);
        i = i.wrapping_add(f.step);
    }
    // Pre-allocate labels and result ids.
    let mut labels: Vec<HashMap<usize, u32>> = Vec::with_capacity(n + 1); // per k: block -> label
    let mut vals: Vec<HashMap<u32, u32>> = Vec::with_capacity(n + 1); // per k: old id -> new id
    for k in 0..=n {
        let mut lm = HashMap::new();
        let mut vm = HashMap::new();
        let blocks_k: Vec<usize> = if k < n { f.blocks.clone() } else { check.clone() };
        for &b in &blocks_k {
            let ob = orig_of(b);
            lm.insert(b, if k == 0 && b == f.h { hl } else { fresh_id(l) });
            for inst in &ob.instructions {
                let Some(r) = inst.result_id else { continue };
                if b == f.h && inst.class.opcode == Op::Phi {
                    continue;
                }
                if Some(r) == f.dead_cond {
                    continue;
                }
                if let Induction::Var { var, ref load_after } = f.induction {
                    if inst.class.opcode == Op::Load && id_op(inst, 0) == Some(var) {
                        vm.insert(r, consts_k[k + usize::from(load_after.contains(&r))]);
                        continue;
                    }
                }
                vm.insert(r, fresh_id(l));
            }
        }
        labels.push(lm);
        vals.push(vm);
    }
    // Header phi values per iteration.
    for k in 0..=n {
        for p in &hphis {
            let pid = p.result_id.unwrap();
            let v = if matches!(f.induction, Induction::Phi { phi } if phi == pid) {
                consts_k[k]
            } else {
                let (v0, l0, v1, _l1) = (id_op(p, 0).unwrap(), id_op(p, 1).unwrap(), id_op(p, 2).unwrap(), id_op(p, 3).unwrap());
                let (init_v, next_v) = if Some(l0) == pl { (v0, v1) } else { (v1, v0) };
                if k == 0 {
                    init_v
                } else {
                    vals[k - 1].get(&next_v).copied().unwrap_or(next_v)
                }
            };
            vals[k].insert(pid, v);
        }
    }
    // Clone.
    let mut new_blocks: Vec<Block> = Vec::new();
    let mut decor_pairs: Vec<(u32, u32)> = Vec::new();
    for k in 0..=n {
        let blocks_k: Vec<usize> = if k < n { f.blocks.clone() } else { check.clone() };
        // Remap for this iteration.
        let mut map: HashMap<u32, u32> = vals[k].clone();
        for (&b, &lab) in &labels[k] {
            map.insert(label_of(orig_of(b)), lab);
        }
        if k < n {
            map.insert(hl, labels[k + 1][&f.h]);
        }
        for &b in &blocks_k {
            let ob = orig_of(b);
            let mut nb = Block::new();
            nb.label = Some(Instruction::new(Op::Label, None, Some(labels[k][&b]), vec![]));
            for inst in &ob.instructions {
                let op = inst.class.opcode;
                if op == Op::LoopMerge || (b == f.h && op == Op::Phi) || (inst.result_id.is_some() && inst.result_id == f.dead_cond) {
                    continue;
                }
                if let Induction::Var { var, .. } = f.induction {
                    if op == Op::Load && id_op(inst, 0) == Some(var) {
                        continue;
                    }
                }
                if b == f.x && op == Op::BranchConditional {
                    let target = if k < n { map[&f.b0] } else { ml };
                    nb.instructions.push(Instruction::new(Op::Branch, None, None, vec![Operand::IdRef(target)]));
                    continue;
                }
                let mut c = inst.clone();
                if let Some(r) = c.result_id {
                    let nr = map[&r];
                    c.result_id = Some(nr);
                    decor_pairs.push((r, nr));
                }
                super::remap_operands(&mut c, &map);
                nb.instructions.push(c);
            }
            new_blocks.push(nb);
        }
    }
    // Splice into the function.
    {
        let func = &mut l.module.functions[fi];
        let end = f.h + f.blocks.len();
        func.blocks.splice(f.h..end, new_blocks);
    }
    // Outside uses of check-block values and header phis -> last iteration; merge-block phis
    // from the exit block -> its last clone.
    let mut outside: HashMap<u32, u32> = HashMap::new();
    for &b in &check {
        for inst in &orig_of(b).instructions {
            if let Some(r) = inst.result_id {
                if let Some(&nr) = vals[n].get(&r) {
                    outside.insert(r, nr);
                }
            }
        }
    }
    let exit_new = labels[n][&f.x];
    {
        let func = &mut l.module.functions[fi];
        for b in func.blocks.iter_mut() {
            let lab = label_of(b);
            if labels.iter().any(|m| m.values().any(|&x| x == lab)) {
                continue; // a cloned block
            }
            for inst in b.instructions.iter_mut() {
                super::remap_operands(inst, &outside);
                if inst.class.opcode == Op::Phi && lab == ml {
                    for o in inst.operands.iter_mut() {
                        if o.id_ref_any() == Some(f.exit_label) {
                            *o = Operand::IdRef(exit_new);
                        }
                    }
                }
            }
        }
    }
    for (from, to) in decor_pairs {
        copy_decorations(l, from, to);
    }
    let mut gone = loop_ids.clone();
    gone.remove(&hl);
    remove_metadata(l, &gone);
    ops.push(EditOp {
        pass: "unroll",
        class: Class::Exact,
        target: hl,
        replaced_by: None,
        detail: format!("{}; {} blocks cloned {} times", f.desc, f.blocks.len(), n),
    });
    l.reanalyze().expect("reanalyze after unroll");
    Some(())
}
