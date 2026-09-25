//! Control-flow graph and dominator tree of one function (Cooper/Harvey/Kennedy iterative
//! algorithm over a reverse postorder).

use super::id_op;
use crate::lift::Lifted;
use rspirv::dr;
use spirv::Op;
use std::collections::HashMap;

pub struct Cfg {
    /// Block labels in module order.
    pub labels: Vec<u32>,
    pub index: HashMap<u32, usize>,
    pub succ: Vec<Vec<usize>>,
    pub pred: Vec<Vec<usize>>,
    /// Immediate dominator (None for the entry and for unreachable blocks).
    pub idom: Vec<Option<usize>>,
    /// Reverse postorder of the reachable blocks.
    pub rpo: Vec<usize>,
}

/// Successor labels of a block's terminator.
pub fn successor_labels(b: &dr::Block) -> Vec<u32> {
    let Some(t) = b.instructions.last() else { return vec![] };
    match t.class.opcode {
        Op::Branch => id_op(t, 0).into_iter().collect(),
        Op::BranchConditional => [id_op(t, 1), id_op(t, 2)].into_iter().flatten().collect(),
        Op::Switch => t.operands.iter().skip(1).filter_map(|o| o.id_ref_any()).collect(),
        _ => vec![],
    }
}

impl Cfg {
    pub fn build(l: &Lifted, fi: usize) -> Cfg {
        let f = &l.module.functions[fi];
        let labels: Vec<u32> = f.blocks.iter().map(|b| b.label.as_ref().and_then(|x| x.result_id).unwrap_or(0)).collect();
        let index: HashMap<u32, usize> = labels.iter().enumerate().map(|(i, x)| (*x, i)).collect();
        let n = labels.len();
        let mut succ = vec![Vec::new(); n];
        let mut pred = vec![Vec::new(); n];
        for (i, b) in f.blocks.iter().enumerate() {
            for s in successor_labels(b) {
                if let Some(&j) = index.get(&s) {
                    if !succ[i].contains(&j) {
                        succ[i].push(j);
                    }
                    if !pred[j].contains(&i) {
                        pred[j].push(i);
                    }
                }
            }
        }
        // Postorder DFS from the entry.
        let mut order = Vec::new();
        let mut state = vec![0u8; n]; // 0 unvisited, 1 in progress, 2 done
        if n > 0 {
            let mut stack: Vec<(usize, usize)> = vec![(0, 0)];
            state[0] = 1;
            while let Some((b, k)) = stack.pop() {
                if k < succ[b].len() {
                    stack.push((b, k + 1));
                    let s = succ[b][k];
                    if state[s] == 0 {
                        state[s] = 1;
                        stack.push((s, 0));
                    }
                } else {
                    state[b] = 2;
                    order.push(b);
                }
            }
        }
        let rpo: Vec<usize> = order.iter().rev().copied().collect();
        let mut rpo_num = vec![usize::MAX; n];
        for (k, b) in rpo.iter().enumerate() {
            rpo_num[*b] = k;
        }
        let mut idom: Vec<Option<usize>> = vec![None; n];
        if n > 0 {
            idom[0] = Some(0);
            let intersect = |idom: &Vec<Option<usize>>, mut a: usize, mut b: usize| {
                while a != b {
                    while rpo_num[a] > rpo_num[b] {
                        a = idom[a].unwrap();
                    }
                    while rpo_num[b] > rpo_num[a] {
                        b = idom[b].unwrap();
                    }
                }
                a
            };
            let mut changed = true;
            while changed {
                changed = false;
                for &b in rpo.iter().skip(1) {
                    let mut new_idom: Option<usize> = None;
                    for &p in &pred[b] {
                        if idom[p].is_none() {
                            continue;
                        }
                        new_idom = Some(match new_idom {
                            None => p,
                            Some(q) => intersect(&idom, p, q),
                        });
                    }
                    if new_idom.is_some() && idom[b] != new_idom {
                        idom[b] = new_idom;
                        changed = true;
                    }
                }
            }
            idom[0] = None;
        }
        Cfg { labels, index, succ, pred, idom, rpo }
    }

    /// True when block `a` dominates block `b` (reflexive).
    pub fn dominates(&self, a: usize, b: usize) -> bool {
        let mut cur = b;
        loop {
            if cur == a {
                return true;
            }
            match self.idom[cur] {
                Some(p) if p != cur => cur = p,
                _ => return false,
            }
        }
    }

    pub fn reachable(&self, b: usize) -> bool {
        b == 0 || self.idom[b].is_some()
    }
}
