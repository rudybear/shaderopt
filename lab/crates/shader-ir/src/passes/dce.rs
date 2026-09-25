//! `dce`: removes pure results with no uses, iterated to a fixed point, and Function-storage
//! variables that are only stored to (together with their stores and access chains).
//! Interface variables (module-scope) are never touched. Ignores `--only-op`.

use super::{describe, has_real_uses, id_op, inst_at, is_pure, real_uses, remove_defs, Class, EditOp, Opts};
use crate::lift::{Lifted, Site};
use spirv::Op;
use std::collections::HashSet;

pub fn run(l: &mut Lifted, ops: &mut Vec<EditOp>, _opts: &Opts) -> usize {
    let mut total = 0;
    loop {
        let n = round(l, ops);
        total += n;
        if n == 0 {
            break;
        }
    }
    total
}

fn round(l: &mut Lifted, ops: &mut Vec<EditOp>) -> usize {
    let mut removed: HashSet<u32> = HashSet::new();
    let mut n = 0;
    // Unused pure results.
    for f in &l.module.functions {
        for b in &f.blocks {
            for inst in &b.instructions {
                let Some(id) = inst.result_id else { continue };
                if inst.class.opcode == Op::Variable {
                    continue; // handled below
                }
                if is_pure(l, inst) && !has_real_uses(l, id) {
                    removed.insert(id);
                    ops.push(EditOp {
                        pass: "dce",
                        class: Class::Exact,
                        target: id,
                        replaced_by: None,
                        detail: format!("{} unused", describe(l, inst)),
                    });
                    n += 1;
                }
            }
        }
    }
    // Function variables only stored to (through access chains as well).
    let mut stores: HashSet<u32> = HashSet::new();
    for f in &l.module.functions {
        for b in &f.blocks {
            for inst in &b.instructions {
                if inst.class.opcode != Op::Variable {
                    continue;
                }
                let id = inst.result_id.unwrap();
                let mut chain: Vec<u32> = Vec::new();
                let mut store_ids: Vec<u32> = Vec::new();
                if only_stored(l, id, &mut chain, &mut store_ids, 0) {
                    let n_stores = store_ids.len();
                    removed.insert(id);
                    removed.extend(chain.iter().copied());
                    // Stores have no result id: remember them by (site) and delete below.
                    stores.extend(store_ids);
                    ops.push(EditOp {
                        pass: "dce",
                        class: Class::Exact,
                        target: id,
                        replaced_by: None,
                        detail: format!(
                            "OpVariable {} only stored to; removed {n_stores} store(s)",
                            l.name(id).map(|s| format!("\"{s}\"")).unwrap_or_default()
                        ),
                    });
                    n += 1;
                }
            }
        }
    }
    if n == 0 {
        return 0;
    }
    // Delete stores whose pointer is one of the removed pointers.
    for f in &mut l.module.functions {
        for b in &mut f.blocks {
            b.instructions.retain(|i| {
                !(i.class.opcode == Op::Store && id_op(i, 0).map_or(false, |p| stores.contains(&p) || removed.contains(&p)))
            });
        }
    }
    remove_defs(l, &removed);
    l.reanalyze().expect("reanalyze after dce");
    n
}

/// True when every real use of pointer `p` is as the target of an `OpStore` or the base of an
/// access chain that is itself only stored to. Collects the chain ids and the pointers stored
/// through.
fn only_stored(l: &Lifted, p: u32, chain: &mut Vec<u32>, stores: &mut Vec<u32>, depth: usize) -> bool {
    if depth > 8 {
        return false;
    }
    let mut any = false;
    for site in real_uses(l, p) {
        let Some(inst) = inst_at(l, *site) else { return false };
        if !matches!(site, Site::Inst(..)) {
            return false;
        }
        match inst.class.opcode {
            Op::Store if id_op(inst, 0) == Some(p) && id_op(inst, 1) != Some(p) => {
                any = true;
                if !stores.contains(&p) {
                    stores.push(p);
                }
            }
            Op::AccessChain | Op::InBoundsAccessChain if id_op(inst, 0) == Some(p) => {
                let c = inst.result_id.unwrap();
                if inst.operands.iter().skip(1).any(|o| o.id_ref_any() == Some(p)) {
                    return false;
                }
                if !only_stored(l, c, chain, stores, depth + 1) {
                    return false;
                }
                chain.push(c);
                any = true;
            }
            _ => return false,
        }
    }
    // A pointer with no uses at all is also dead (an unused variable).
    any || real_uses(l, p).next().is_none()
}
