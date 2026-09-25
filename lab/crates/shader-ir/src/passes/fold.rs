//! `fold`: constant folding of arithmetic, comparisons, composites, conversions, `OpSelect`
//! with a constant condition and GLSL.std.450 with all-constant operands. Results become
//! `OpConstant`/`OpConstantComposite` in the global section (deduplicated). See
//! [`super::consts`] for the precision and class rules.

use super::consts::{self, fmt_cv, CV};
use super::{describe, glsl_op, remove_defs, replace_uses, Class, EditOp, Opts};
use crate::lift::Lifted;
use spirv::Op;
use std::collections::HashSet;

pub fn run(l: &mut Lifted, ops: &mut Vec<EditOp>, opts: &Opts) -> usize {
    let mut total = 0;
    loop {
        let n = round(l, ops, opts);
        total += n;
        if n == 0 {
            break;
        }
    }
    total
}

struct Cand {
    id: u32,
    ty: u32,
    value: Option<CV>,
    /// `OpSelect` with a constant condition: the chosen operand id.
    forward: Option<u32>,
    class: Class,
    desc: String,
}

fn round(l: &mut Lifted, ops: &mut Vec<EditOp>, opts: &Opts) -> usize {
    let mut cands: Vec<Cand> = Vec::new();
    for f in &l.module.functions {
        for b in &f.blocks {
            for inst in &b.instructions {
                let (Some(id), Some(ty)) = (inst.result_id, inst.result_type) else { continue };
                if !opts.allows(id) {
                    continue;
                }
                let op = inst.class.opcode;
                let is_ext = op == Op::ExtInst;
                if is_ext && glsl_op(l, inst).is_none() {
                    continue;
                }
                let ids: Vec<u32> = inst.operands.iter().skip(if is_ext { 2 } else { 0 }).filter_map(|o| o.id_ref_any()).collect();
                if ids.is_empty() {
                    continue;
                }
                // OpSelect with a constant scalar condition forwards an operand even when the
                // arms are not constants.
                if op == Op::Select && ids.len() == 3 {
                    if let Some(CV::Bool(c)) = consts::read(l, ids[0]) {
                        if consts::read(l, ids[1]).is_none() || consts::read(l, ids[2]).is_none() {
                            let chosen = if c { ids[1] } else { ids[2] };
                            cands.push(Cand {
                                id,
                                ty,
                                value: None,
                                forward: Some(chosen),
                                class: Class::Exact,
                                desc: format!("{} -> %{chosen}", describe(l, inst)),
                            });
                            continue;
                        }
                    }
                }
                let args: Option<Vec<CV>> = ids.iter().map(|i| consts::read(l, *i)).collect();
                let Some(args) = args else { continue };
                let Some(folded) = consts::eval(l, inst, &args) else { continue };
                if opts.exact_only && folded.class != Class::Exact {
                    continue;
                }
                cands.push(Cand {
                    id,
                    ty,
                    forward: None,
                    class: folded.class,
                    desc: describe(l, inst),
                    value: Some(folded.value),
                });
            }
        }
    }
    if cands.is_empty() {
        return 0;
    }
    let mut removed = HashSet::new();
    let mut n = 0;
    for c in cands {
        let (new_id, detail) = if let Some(fw) = c.forward {
            (fw, c.desc)
        } else {
            let v = c.value.as_ref().unwrap();
            let Some(cid) = consts::intern(l, c.ty, v) else { continue };
            let kind = if matches!(v, CV::Comp(_)) { "OpConstantComposite" } else { "OpConstant" };
            (cid, format!("{} -> {kind} {}", c.desc, fmt_cv(v)))
        };
        replace_uses(l, c.id, new_id);
        removed.insert(c.id);
        ops.push(EditOp { pass: "fold", class: c.class, target: c.id, replaced_by: Some(new_id), detail });
        n += 1;
    }
    remove_defs(l, &removed);
    l.reanalyze().expect("reanalyze after fold");
    n
}
