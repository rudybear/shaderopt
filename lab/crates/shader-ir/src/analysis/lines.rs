//! Source-line mapping through a `glslang -V -g` build of the same shader.
//!
//! The debug build's function bodies, after dropping `OpLine`/`OpNoLine`, are the same opcode
//! sequence as the measured build (`CONTRACTS.md`, "Debug builds for source mapping"), so the
//! k-th body instruction of the measured build maps to the k-th of the debug build and thereby
//! to the line of the last `OpLine` before it. Result ids differ between the builds and are never
//! compared; the opcode sequence (and `GLSL.std.450` instruction numbers) must match exactly,
//! otherwise mapping fails with an error naming the first mismatch.

use super::Body;
use crate::lift::Lifted;
use anyhow::{bail, Result};
use rspirv::dr::{self, Operand};
use spirv::Op;

fn ext_num(inst: &dr::Instruction) -> Option<u32> {
    if inst.class.opcode != Op::ExtInst {
        return None;
    }
    match inst.operands.get(1) {
        Some(Operand::LiteralExtInstInteger(n)) => Some(*n),
        _ => None,
    }
}

/// One line per body instruction of `body`, from `dbg`.
pub fn map_lines(body: &Body<'_>, dbg: &Lifted) -> Result<Vec<Option<u32>>> {
    // Flatten the debug build the same way `Body` does, carrying the current line.
    let mut seq: Vec<(usize, Op, Option<u32>, Option<u32>)> = Vec::new(); // (function, op, ext, line)
    for (fi, f) in dbg.module.functions.iter().enumerate() {
        let mut line: Option<u32> = None;
        for p in &f.parameters {
            seq.push((fi, p.class.opcode, None, line));
        }
        for b in &f.blocks {
            if let Some(l) = &b.label {
                seq.push((fi, l.class.opcode, None, line));
            }
            for inst in &b.instructions {
                match inst.class.opcode {
                    Op::Line => {
                        line = match inst.operands.get(1) {
                            Some(Operand::LiteralBit32(n)) => Some(*n),
                            _ => None,
                        };
                    }
                    Op::NoLine => line = None,
                    op => seq.push((fi, op, ext_num(inst), line)),
                }
            }
        }
    }
    if dbg.module.functions.len() != body.lifted.module.functions.len() {
        bail!(
            "debug build has {} functions, measured build {}",
            dbg.module.functions.len(),
            body.lifted.module.functions.len()
        );
    }
    if seq.len() != body.insts.len() {
        bail!(
            "debug build has {} body instructions (without OpLine/OpNoLine), measured build {}; the builds do not correspond",
            seq.len(),
            body.insts.len()
        );
    }
    let mut out = Vec::with_capacity(seq.len());
    for (k, (bi, d)) in body.insts.iter().zip(&seq).enumerate() {
        let op = bi.inst.class.opcode;
        if op != d.1 || bi.fi != d.0 || ext_num(bi.inst) != d.2 {
            bail!(
                "debug build does not match at body instruction {k}: measured Op{} (function %{}, index {}) vs debug Op{:?}",
                bi.inst.class.opname,
                body.lifted.functions[bi.fi].id,
                bi.index,
                d.1
            );
        }
        out.push(d.3);
    }
    Ok(out)
}
