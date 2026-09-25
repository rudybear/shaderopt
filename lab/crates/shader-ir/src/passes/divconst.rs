//! `divconst`: `x / c -> x * (1/c)` for a constant scalar or vector `c` whose every component
//! is finite, non-zero and normal, and whose reciprocal is finite and normal. The instruction is
//! rewritten in place (same result id). Class `ulp`; the detail carries the exact reciprocal
//! bits. When every component of `c` is a power of two the product is bit-identical to the
//! quotient and the class is `exact`.

use super::consts::{self, CV};
use super::{describe, float_width, id_op, Class, EditOp, Opts};
use crate::lift::Lifted;
use rspirv::dr::Operand;
use spirv::Op;

pub fn run(l: &mut Lifted, ops: &mut Vec<EditOp>, opts: &Opts) -> usize {
    // (function, block, index, id, c id, reciprocal value, class, detail)
    let mut edits: Vec<(usize, usize, usize, u32, CV, Class, String)> = Vec::new();
    for (fi, f) in l.module.functions.iter().enumerate() {
        for (bi, b) in f.blocks.iter().enumerate() {
            for (ii, inst) in b.instructions.iter().enumerate() {
                if inst.class.opcode != Op::FDiv {
                    continue;
                }
                let (Some(id), Some(ty)) = (inst.result_id, inst.result_type) else { continue };
                if !opts.allows(id) {
                    continue;
                }
                let Some(c) = id_op(inst, 1) else { continue };
                let Some(width) = float_width(l, ty) else { continue };
                let Some(cv) = consts::read(l, c) else { continue };
                let comps = cv.comps();
                let mut recips = Vec::with_capacity(comps.len());
                let mut all_pow2 = true;
                let mut bits = Vec::new();
                let mut ok = true;
                for comp in &comps {
                    match (width, comp) {
                        (32, CV::F32(v)) => {
                            let r = 1.0f32 / v;
                            if !v.is_normal() || !r.is_normal() {
                                ok = false;
                                break;
                            }
                            all_pow2 &= v.abs().log2().fract() == 0.0 && (r * v == 1.0);
                            bits.push(format!("{:#010x}", r.to_bits()));
                            recips.push(CV::F32(r));
                        }
                        (64, CV::F64(v)) => {
                            let r = 1.0f64 / v;
                            if !v.is_normal() || !r.is_normal() {
                                ok = false;
                                break;
                            }
                            all_pow2 &= v.abs().log2().fract() == 0.0 && (r * v == 1.0);
                            bits.push(format!("{:#018x}", r.to_bits()));
                            recips.push(CV::F64(r));
                        }
                        _ => {
                            ok = false;
                            break;
                        }
                    }
                }
                if !ok || recips.is_empty() {
                    continue;
                }
                let recip = if matches!(cv, CV::Comp(_)) { CV::Comp(recips) } else { recips.into_iter().next().unwrap() };
                let class = if all_pow2 { Class::Exact } else { Class::Ulp };
                if opts.exact_only && class != Class::Exact {
                    continue;
                }
                let detail = format!(
                    "{} (c = {}) -> OpFMul %{} %r (r = 1/c = {}, bits {}){}",
                    describe(l, inst),
                    consts::fmt_cv(&cv),
                    id_op(inst, 0).unwrap_or(0),
                    consts::fmt_cv(&recip),
                    bits.join(","),
                    if all_pow2 { "; power of two: exact" } else { "" }
                );
                edits.push((fi, bi, ii, id, recip, class, detail));
            }
        }
    }
    if edits.is_empty() {
        return 0;
    }
    let n = edits.len();
    for (fi, bi, ii, id, recip, class, detail) in edits {
        let ty = l.module.functions[fi].blocks[bi].instructions[ii].result_type.unwrap();
        let cty = {
            let c = id_op(&l.module.functions[fi].blocks[bi].instructions[ii], 1).unwrap();
            l.constants[&c].ty
        };
        let Some(rid) = consts::intern(l, cty, &recip) else { continue };
        let inst = &mut l.module.functions[fi].blocks[bi].instructions[ii];
        let x = id_op(inst, 0).unwrap();
        *inst = rspirv::dr::Instruction::new(Op::FMul, Some(ty), Some(id), vec![Operand::IdRef(x), Operand::IdRef(rid)]);
        ops.push(EditOp { pass: "divconst", class, target: id, replaced_by: Some(id), detail: detail.replace("%r ", &format!("%{rid} ")) });
    }
    l.reanalyze().expect("reanalyze after divconst");
    n
}
