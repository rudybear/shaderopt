//! `ident`: algebraic identities that are bit-exact under IEEE semantics:
//! `x*1 -> x`, `1*x`, `x+0`, `0+x`, `x-0`, `x/1`, `-(-x)`, `select(c,x,x)`, vector forms with
//! splat constants and `OpVectorTimesScalar(v, 1.0)`. Integer forms as well.
//! Not done: `x*0` and `x-x` (NaN/Inf semantics).
//!
//! Caveat recorded in the op detail: `x + (+0.0) -> x` maps `-0.0` to `+0.0` before the rewrite
//! and to `-0.0` after; the contract lists the identity as `exact` and the lab's images never
//! distinguish the sign of zero, but it is the one identity here that is not bit-exact on every
//! input. `x + (-0.0)`, `x - (+0.0)`, `x * 1`, `x / 1` and `-(-x)` are exact for every x.

use super::consts::{self, CV};
use super::{def_inst, describe, id_op, remove_defs, replace_uses, Class, EditOp, Opts};
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

fn is_one(l: &Lifted, id: u32) -> bool {
    consts::read(l, id).map_or(false, |c| c.is_splat_f(1.0) || c.is_splat_i(1))
}

/// `Some(is_negative_zero_free)`: `Some(true)` for integer 0 or float -0.0, `Some(false)` for
/// float +0.0 (the sign-of-zero caveat), `None` when not a zero.
fn zero_kind(l: &Lifted, id: u32) -> Option<bool> {
    let c = consts::read(l, id)?;
    let comps = c.comps();
    if comps.is_empty() {
        return None;
    }
    if comps.iter().all(|x| x.as_i() == Some(0)) {
        return Some(true);
    }
    if comps.iter().all(|x| x.as_f64() == Some(0.0)) {
        let all_neg = comps.iter().all(|x| match x {
            CV::F32(f) => f.is_sign_negative(),
            CV::F64(f) => f.is_sign_negative(),
            _ => false,
        });
        return Some(all_neg);
    }
    None
}

fn round(l: &mut Lifted, ops: &mut Vec<EditOp>, opts: &Opts) -> usize {
    // (target, replacement, detail)
    let mut edits: Vec<(u32, u32, String)> = Vec::new();
    for f in &l.module.functions {
        for b in &f.blocks {
            for inst in &b.instructions {
                let Some(id) = inst.result_id else { continue };
                if !opts.allows(id) {
                    continue;
                }
                let a = id_op(inst, 0);
                let bb = id_op(inst, 1);
                let (Some(a), Some(bb)) = (a, bb.or(a)) else { continue };
                let mut hit: Option<(u32, &str)> = None;
                match inst.class.opcode {
                    Op::FMul | Op::IMul => {
                        if is_one(l, bb) {
                            hit = Some((a, "x*1"));
                        } else if is_one(l, a) {
                            hit = Some((bb, "1*x"));
                        }
                    }
                    Op::VectorTimesScalar => {
                        if is_one(l, bb) {
                            hit = Some((a, "v*1"));
                        }
                    }
                    Op::FAdd | Op::IAdd => {
                        if let Some(clean) = zero_kind(l, bb) {
                            hit = Some((a, if clean { "x+0" } else { "x+0 (sign of -0.0 not preserved)" }));
                        } else if let Some(clean) = zero_kind(l, a) {
                            hit = Some((bb, if clean { "0+x" } else { "0+x (sign of -0.0 not preserved)" }));
                        }
                    }
                    Op::FSub | Op::ISub => {
                        if let Some(clean) = zero_kind(l, bb) {
                            // x - (+0.0) is exact for every x; x - (-0.0) maps -0.0 to +0.0.
                            let is_float = inst.class.opcode == Op::FSub;
                            hit = Some((a, if !is_float || !clean { "x-0" } else { "x-(-0) (sign of -0.0 not preserved)" }));
                        }
                    }
                    Op::FDiv | Op::SDiv | Op::UDiv => {
                        if is_one(l, bb) {
                            hit = Some((a, "x/1"));
                        }
                    }
                    Op::FNegate | Op::SNegate => {
                        if let Some(inner) = def_inst(l, a) {
                            if inner.class.opcode == inst.class.opcode {
                                if let Some(x) = id_op(inner, 0) {
                                    hit = Some((x, "-(-x)"));
                                }
                            }
                        }
                    }
                    Op::Select => {
                        if let (Some(x), Some(y)) = (id_op(inst, 1), id_op(inst, 2)) {
                            if x == y {
                                hit = Some((x, "select(c,x,x)"));
                            }
                        }
                    }
                    _ => {}
                }
                if let Some((rep, what)) = hit {
                    edits.push((id, rep, format!("{} -> %{rep} [{what}]", describe(l, inst))));
                }
            }
        }
    }
    if edits.is_empty() {
        return 0;
    }
    let mut removed = HashSet::new();
    let n = edits.len();
    for (id, mut rep, detail) in edits {
        // Chains within one round: if the replacement was itself replaced, follow it.
        let mut guard = 0;
        while let Some(o) = ops.iter().rev().find(|o| o.pass == "ident" && o.target == rep && removed.contains(&rep)) {
            rep = o.replaced_by.unwrap();
            guard += 1;
            if guard > 16 {
                break;
            }
        }
        replace_uses(l, id, rep);
        removed.insert(id);
        ops.push(EditOp { pass: "ident", class: Class::Exact, target: id, replaced_by: Some(rep), detail });
    }
    remove_defs(l, &removed);
    l.reanalyze().expect("reanalyze after ident");
    n
}
