//! `cse`: within a function, identical pure instructions (same opcode, result type, operands
//! and ext-inst) where the earlier dominates the later are merged into the earlier. Dominance
//! comes from a dominator tree over the CFG. Loads qualify only from read-only storage
//! (Uniform, UniformConstant, PushConstant, Input); image operations and derivatives are left
//! alone.

use super::cfg::Cfg;
use super::{describe, is_cse_candidate, remove_defs, remove_metadata, replace_uses, Class, EditOp, Opts};
use crate::lift::Lifted;
use std::collections::{HashMap, HashSet};

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

fn round(l: &mut Lifted, ops: &mut Vec<EditOp>, opts: &Opts) -> usize {
    // (later id, earlier id, detail)
    let mut merges: Vec<(u32, u32, String)> = Vec::new();
    for fi in 0..l.module.functions.len() {
        let cfg = Cfg::build(l, fi);
        // Children in the dominator tree.
        let n = cfg.labels.len();
        let mut children: Vec<Vec<usize>> = vec![Vec::new(); n];
        for b in 0..n {
            if let Some(p) = cfg.idom[b] {
                children[p].push(b);
            }
        }
        // DFS over the dominator tree with a scoped table of available expressions.
        let mut subst: HashMap<u32, u32> = HashMap::new();
        let mut scopes: Vec<Vec<String>> = Vec::new();
        let mut table: HashMap<String, u32> = HashMap::new();
        let mut stack: Vec<(usize, bool)> = vec![(0, false)];
        while let Some((b, leaving)) = stack.pop() {
            if leaving {
                for k in scopes.pop().unwrap_or_default() {
                    table.remove(&k);
                }
                continue;
            }
            stack.push((b, true));
            scopes.push(Vec::new());
            let block = &l.module.functions[fi].blocks[b];
            for inst in &block.instructions {
                let Some(id) = inst.result_id else { continue };
                if !is_cse_candidate(l, inst) {
                    continue;
                }
                let key = {
                    let mut k = format!("{:?}|{:?}|", inst.class.opcode, inst.result_type);
                    for o in &inst.operands {
                        match o.id_ref_any() {
                            Some(x) => k.push_str(&format!("%{} ", subst.get(&x).copied().unwrap_or(x))),
                            None => k.push_str(&format!("{o:?} ")),
                        }
                    }
                    k
                };
                if let Some(&earlier) = table.get(&key) {
                    if opts.allows(id) {
                        subst.insert(id, earlier);
                        let d = describe(l, inst);
                        merges.push((id, earlier, format!("{d} == %{earlier}")));
                        continue;
                    }
                }
                table.insert(key.clone(), id);
                scopes.last_mut().unwrap().push(key);
            }
            for &c in children[b].iter().rev() {
                stack.push((c, false));
            }
        }
    }
    if merges.is_empty() {
        return 0;
    }
    let mut removed = HashSet::new();
    let n = merges.len();
    for (later, earlier, detail) in merges {
        replace_uses(l, later, earlier);
        removed.insert(later);
        ops.push(EditOp { pass: "cse", class: Class::Exact, target: later, replaced_by: Some(earlier), detail });
    }
    remove_defs(l, &removed);
    remove_metadata(l, &removed);
    l.reanalyze().expect("reanalyze after cse");
    n
}
