//! Per-function control-flow facts: successors, dominators, post-dominators and control
//! dependence. Functions are small (tens to a few hundred blocks), so dominator sets are kept as
//! plain boolean matrices computed by the textbook fixed point; simplicity over speed.

use rspirv::dr::{self, Operand};
use spirv::Op;

pub struct Cfg {
    pub n: usize,
    pub succ: Vec<Vec<usize>>,
    pub pred: Vec<Vec<usize>>,
    /// `dom[b][a]`: block `a` dominates block `b` (reflexive).
    pub dom: Vec<Vec<bool>>,
    /// `pdom[b][a]`: block `a` post-dominates block `b` (reflexive); index `n` is the virtual exit.
    pub pdom: Vec<Vec<bool>>,
    /// `ctrl_deps[b]`: blocks with a conditional terminator that `b` is control dependent on.
    pub ctrl_deps: Vec<Vec<usize>>,
    /// The condition/selector id of each block's terminator, when it is conditional.
    pub cond: Vec<Option<u32>>,
    /// Blocks ending in a return/kill/unreachable.
    pub exit: Vec<bool>,
}

/// Label ids of the successors of a block, in terminator order.
pub fn successors(term: &dr::Instruction) -> Vec<u32> {
    match term.class.opcode {
        Op::Branch => term.operands.iter().filter_map(|o| o.id_ref_any()).collect(),
        Op::BranchConditional => term.operands.iter().skip(1).take(2).filter_map(|o| o.id_ref_any()).collect(),
        Op::Switch => {
            let mut out = Vec::new();
            if let Some(Operand::IdRef(d)) = term.operands.get(1) {
                out.push(*d);
            }
            let mut i = 2;
            while i + 1 < term.operands.len() {
                if let Operand::IdRef(l) = &term.operands[i + 1] {
                    out.push(*l);
                }
                i += 2;
            }
            out
        }
        _ => Vec::new(),
    }
}

impl Cfg {
    pub fn build(f: &dr::Function, label_index: &std::collections::HashMap<u32, usize>) -> Cfg {
        let n = f.blocks.len();
        let mut succ = vec![Vec::new(); n];
        let mut pred = vec![Vec::new(); n];
        let mut cond = vec![None; n];
        let mut exit = vec![false; n];
        for (b, blk) in f.blocks.iter().enumerate() {
            let Some(term) = blk.instructions.last() else { continue };
            match term.class.opcode {
                Op::BranchConditional | Op::Switch => cond[b] = term.operands.first().and_then(|o| o.id_ref_any()),
                Op::Return | Op::ReturnValue | Op::Kill | Op::TerminateInvocation | Op::Unreachable => exit[b] = true,
                _ => {}
            }
            for l in successors(term) {
                if let Some(&s) = label_index.get(&l) {
                    if !succ[b].contains(&s) {
                        succ[b].push(s);
                        pred[s].push(b);
                    }
                }
            }
            if succ[b].is_empty() {
                exit[b] = true;
            }
        }
        let dom = dominator_sets(n, 0, &pred);
        // Post-dominators: reverse graph with a virtual exit node `n`.
        let mut rsucc = vec![Vec::new(); n + 1]; // predecessors in the reverse graph = successors
        for b in 0..n {
            rsucc[b] = succ[b].clone();
            if exit[b] {
                rsucc[b].push(n);
            }
        }
        let pdom = dominator_sets(n + 1, n, &rsucc);
        let mut ctrl_deps = vec![Vec::new(); n];
        for a in 0..n {
            if cond[a].is_none() {
                continue;
            }
            for b in 0..n {
                // b is control dependent on a iff some successor s of a is post-dominated by b
                // and b does not strictly post-dominate a.
                let strictly_pdom_a = b != a && pdom[a][b];
                if strictly_pdom_a {
                    continue;
                }
                if succ[a].iter().any(|&s| pdom[s][b]) {
                    ctrl_deps[b].push(a);
                }
            }
        }
        Cfg { n, succ, pred, dom, pdom, ctrl_deps, cond, exit }
    }

    /// `a` dominates `b`.
    pub fn dominates(&self, a: usize, b: usize) -> bool {
        self.dom[b][a]
    }
}

/// `sets[b][a]` = a dominates b, for the graph given by `preds` with root `root`.
fn dominator_sets(n: usize, root: usize, preds: &[Vec<usize>]) -> Vec<Vec<bool>> {
    let mut sets = vec![vec![true; n]; n];
    sets[root] = vec![false; n];
    sets[root][root] = true;
    let mut changed = true;
    while changed {
        changed = false;
        for b in 0..n {
            if b == root {
                continue;
            }
            let mut new = vec![true; n];
            let mut any = false;
            for &p in &preds[b] {
                any = true;
                for a in 0..n {
                    new[a] = new[a] && sets[p][a];
                }
            }
            if !any {
                // Unreachable block: leave it dominated by everything (dead code).
                continue;
            }
            new[b] = true;
            if new != sets[b] {
                sets[b] = new;
                changed = true;
            }
        }
    }
    sets
}
