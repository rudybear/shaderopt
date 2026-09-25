//! `powspec`: GLSL.std.450 `Pow(x, e)` with a constant scalar/splat exponent:
//! `e == 2 -> x*x`, `e == 0.5 -> Sqrt(x)`, `e == 1 -> x`, `e == 3 -> (x*x)*x`. Class `ulp`.
//! `pow` is undefined for negative `x` while the products are defined; this only widens the
//! defined domain. `exp(log(x)*c)` is untouched. The rewritten instruction keeps its id.

use super::consts;
use super::{describe, fresh_id, glsl_op, id_op, remove_defs, replace_uses, Class, EditOp, Opts};
use crate::lift::Lifted;
use rspirv::dr::{Instruction, Operand};
use spirv::{GLOp, Op};
use std::collections::HashSet;

enum Kind {
    Square,
    Sqrt,
    One,
    Cube,
}

pub fn run(l: &mut Lifted, ops: &mut Vec<EditOp>, opts: &Opts) -> usize {
    if opts.exact_only {
        return 0;
    }
    let mut edits: Vec<(usize, usize, usize, u32, Kind, String)> = Vec::new();
    for (fi, f) in l.module.functions.iter().enumerate() {
        for (bi, b) in f.blocks.iter().enumerate() {
            for (ii, inst) in b.instructions.iter().enumerate() {
                if glsl_op(l, inst) != Some(GLOp::Pow) {
                    continue;
                }
                let Some(id) = inst.result_id else { continue };
                if !opts.allows(id) {
                    continue;
                }
                let (Some(x), Some(e)) = (id_op(inst, 2), id_op(inst, 3)) else { continue };
                let Some(ev) = consts::read(l, e) else { continue };
                let kind = if ev.is_splat_f(2.0) {
                    Kind::Square
                } else if ev.is_splat_f(0.5) {
                    Kind::Sqrt
                } else if ev.is_splat_f(1.0) {
                    Kind::One
                } else if ev.is_splat_f(3.0) {
                    Kind::Cube
                } else {
                    continue;
                };
                let what = match kind {
                    Kind::Square => format!("OpFMul %{x} %{x}"),
                    Kind::Sqrt => format!("OpExtInst Sqrt %{x}"),
                    Kind::One => format!("%{x}"),
                    Kind::Cube => format!("OpFMul (OpFMul %{x} %{x}) %{x}"),
                };
                edits.push((fi, bi, ii, id, kind, format!("{} (e = {}) -> {what}", describe(l, inst), consts::fmt_cv(&ev))));
            }
        }
    }
    if edits.is_empty() {
        return 0;
    }
    let n = edits.len();
    let mut removed = HashSet::new();
    // Process in reverse so that inserted instructions do not shift later indices.
    for (fi, bi, ii, id, kind, detail) in edits.into_iter().rev() {
        let inst = l.module.functions[fi].blocks[bi].instructions[ii].clone();
        let ty = inst.result_type.unwrap();
        let set = id_op(&inst, 0).unwrap();
        let x = id_op(&inst, 2).unwrap();
        let replaced_by = match kind {
            Kind::Square => {
                l.module.functions[fi].blocks[bi].instructions[ii] =
                    Instruction::new(Op::FMul, Some(ty), Some(id), vec![Operand::IdRef(x), Operand::IdRef(x)]);
                Some(id)
            }
            Kind::Sqrt => {
                l.module.functions[fi].blocks[bi].instructions[ii] = Instruction::new(
                    Op::ExtInst,
                    Some(ty),
                    Some(id),
                    vec![Operand::IdRef(set), Operand::LiteralExtInstInteger(GLOp::Sqrt as u32), Operand::IdRef(x)],
                );
                Some(id)
            }
            Kind::One => {
                replace_uses(l, id, x);
                removed.insert(id);
                Some(x)
            }
            Kind::Cube => {
                let t = fresh_id(l);
                let sq = Instruction::new(Op::FMul, Some(ty), Some(t), vec![Operand::IdRef(x), Operand::IdRef(x)]);
                let insts = &mut l.module.functions[fi].blocks[bi].instructions;
                insts[ii] = Instruction::new(Op::FMul, Some(ty), Some(id), vec![Operand::IdRef(t), Operand::IdRef(x)]);
                insts.insert(ii, sq);
                Some(id)
            }
        };
        ops.push(EditOp { pass: "powspec", class: Class::Ulp, target: id, replaced_by, detail });
    }
    remove_defs(l, &removed);
    l.reanalyze().expect("reanalyze after powspec");
    n
}
