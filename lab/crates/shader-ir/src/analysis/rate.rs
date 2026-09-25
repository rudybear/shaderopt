//! Rate analysis: forward dataflow over `const < uniform < pixel`.
//!
//! * Constants, `OpVariable` pointers and unknown module-level ids are `const`.
//! * `OpLoad` from a `Uniform`/`PushConstant`/`UniformConstant` variable is `uniform`, from an
//!   `Input` variable (Location inputs, `gl_FragCoord`, every builtin) `pixel`, from a
//!   `Function`/`Private`/`Output` variable the maximum over every store to that variable (all
//!   stores are treated as reaching) joined with the control rate of the storing block; access
//!   chain index rates are joined in.
//! * Image sample/fetch/gather/query results and derivatives are `pixel`.
//! * `OpPhi` joins its incoming values with the control rate of each predecessor and the
//!   condition of a predecessor's conditional branch (divergent control flow).
//! * Every other instruction is the join of its id operands.
//! * The *control rate* of a block is the join of the branch conditions it is control dependent
//!   on (see [`super::cfg`]); a store under a pixel-rate `if` makes the variable pixel-rate.
//! * `OpFunctionCall` analyzes the callee per call site with the argument rates (pointer
//!   arguments alias the caller's variables and report the rate of the aliased storage); the
//!   callee's instructions are reported once with the join over call sites. Functions never
//!   called are analyzed with `pixel` parameters.
//!
//! The whole thing iterates to a fixed point: inside each function until no result changes, and
//! over the module until no variable rate changes.

use super::Body;
use anyhow::{bail, Result};
use spirv::{Op, StorageClass};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Rate {
    Const,
    Uniform,
    Pixel,
}

impl Rate {
    pub fn as_str(self) -> &'static str {
        match self {
            Rate::Const => "const",
            Rate::Uniform => "uniform",
            Rate::Pixel => "pixel",
        }
    }
}

pub struct Rates {
    /// Result id -> rate (joined over call sites).
    pub ids: HashMap<u32, Rate>,
    /// Variable id -> rate of its contents.
    pub vars: HashMap<u32, Rate>,
}

impl Rates {
    pub fn rate_of_id(&self, id: u32) -> Rate {
        self.ids.get(&id).copied().unwrap_or(Rate::Const)
    }

    /// Rate of body instruction `k`: its result's rate, or the join of its operands when it has
    /// no result (stores, branches).
    pub fn inst_rate(&self, k: usize, body: &Body<'_>) -> Rate {
        let inst = body.insts[k].inst;
        match inst.result_id {
            Some(id) => self.rate_of_id(id),
            None => Body::id_operands(inst).iter().map(|o| self.rate_of_id(*o)).max().unwrap_or(Rate::Const),
        }
    }
}

struct Ctx<'b, 'a> {
    body: &'b Body<'a>,
    ids: HashMap<u32, Rate>,
    vars: HashMap<u32, Rate>,
    changed: bool,
    stack: Vec<usize>,
    visited: Vec<bool>,
}

fn is_image_op(op: Op, name: &str) -> bool {
    !matches!(op, Op::Image | Op::SampledImage | Op::TypeImage | Op::TypeSampledImage) && name.starts_with("Image")
}

fn is_derivative(name: &str) -> bool {
    name.starts_with("DPd") || name.starts_with("Fwidth")
}

impl<'b, 'a> Ctx<'b, 'a> {
    fn raise_var(&mut self, var: u32, r: Rate) {
        let e = self.vars.entry(var).or_insert(Rate::Const);
        if r > *e {
            *e = r;
            self.changed = true;
        }
    }

    fn analyze_fn(&mut self, fi: usize, param_rates: &[Rate], param_roots: &[Option<u32>]) -> Result<Rate> {
        if self.stack.contains(&fi) {
            bail!("recursive call to function %{} is not supported", self.body.lifted.functions[fi].id);
        }
        self.stack.push(fi);
        self.visited[fi] = true;
        let body = self.body;
        let l = body.lifted;
        let f = &l.module.functions[fi];
        let info = &l.functions[fi];
        let cfg = &body.cfgs[fi];
        let mut local: HashMap<u32, Rate> = HashMap::new();
        let mut roots: HashMap<u32, Option<u32>> = HashMap::new();
        for (i, p) in info.params.iter().enumerate() {
            local.insert(*p, param_rates.get(i).copied().unwrap_or(Rate::Pixel));
            roots.insert(*p, param_roots.get(i).copied().flatten());
        }
        let mut ret = Rate::Const;
        for _pass in 0..1000 {
            let mut local_changed = false;
            ret = Rate::Const;
            // A pointer parameter reports the rate of the storage it aliases.
            for (i, p) in info.params.iter().enumerate() {
                if let Some(Some(root)) = param_roots.get(i) {
                    let r = self.vars.get(root).copied().unwrap_or(Rate::Const);
                    let e = local.entry(*p).or_insert(Rate::Const);
                    if r > *e {
                        *e = r;
                    }
                }
            }
            let rate_of = |local: &HashMap<u32, Rate>, id: u32| local.get(&id).copied().unwrap_or(Rate::Const);
            let root_of = |roots: &HashMap<u32, Option<u32>>, id: u32| -> Option<u32> {
                let mut cur = id;
                for _ in 0..64 {
                    if let Some(r) = roots.get(&cur) {
                        return *r;
                    }
                    if l.variables.iter().any(|v| v.id == cur) {
                        return Some(cur);
                    }
                    let d = body.def(cur)?;
                    match d.inst.class.opcode {
                        Op::Variable => return Some(cur),
                        Op::AccessChain | Op::InBoundsAccessChain | Op::PtrAccessChain | Op::CopyObject | Op::CopyLogical => {
                            cur = d.inst.operands.first().and_then(|o| o.id_ref_any())?;
                        }
                        _ => return None,
                    }
                }
                None
            };
            // Control rate of every block from the conditions it depends on.
            let ctrl: Vec<Rate> = (0..cfg.n)
                .map(|b| cfg.ctrl_deps[b].iter().filter_map(|&a| cfg.cond[a]).map(|c| rate_of(&local, c)).max().unwrap_or(Rate::Const))
                .collect();
            for (bi, blk) in f.blocks.iter().enumerate() {
                for inst in &blk.instructions {
                    let op = inst.class.opcode;
                    let ops = Body::id_operands(inst);
                    let join = |local: &HashMap<u32, Rate>| ops.iter().map(|o| rate_of(local, *o)).max().unwrap_or(Rate::Const);
                    let load_rate = |local: &HashMap<u32, Rate>, roots: &HashMap<u32, Option<u32>>, vars: &HashMap<u32, Rate>, ptr: u32| -> Rate {
                        let base = match root_of(roots, ptr) {
                            None => Rate::Pixel,
                            Some(root) => match body.storage_of_var(root) {
                                Some(StorageClass::Uniform) | Some(StorageClass::PushConstant) | Some(StorageClass::UniformConstant) => Rate::Uniform,
                                Some(StorageClass::Input) => Rate::Pixel,
                                Some(StorageClass::Function) | Some(StorageClass::Private) | Some(StorageClass::Output) | Some(StorageClass::Workgroup) => {
                                    vars.get(&root).copied().unwrap_or(Rate::Const)
                                }
                                _ => Rate::Pixel,
                            },
                        };
                        base.max(rate_of(local, ptr))
                    };
                    let mut result: Option<Rate> = None;
                    match op {
                        Op::Nop | Op::Line | Op::NoLine | Op::SelectionMerge | Op::LoopMerge | Op::Branch | Op::Return | Op::Kill
                        | Op::TerminateInvocation | Op::DemoteToHelperInvocation | Op::Unreachable | Op::BranchConditional | Op::Switch => {}
                        Op::Variable => {
                            roots.insert(inst.result_id.unwrap(), Some(inst.result_id.unwrap()));
                            result = Some(Rate::Const);
                        }
                        Op::Load => {
                            result = Some(load_rate(&local, &roots, &self.vars, ops[0]));
                        }
                        Op::Store => {
                            let r = rate_of(&local, ops[1]).max(rate_of(&local, ops[0])).max(ctrl[bi]);
                            if let Some(root) = root_of(&roots, ops[0]) {
                                self.raise_var(root, r);
                            }
                        }
                        Op::CopyMemory => {
                            let r = load_rate(&local, &roots, &self.vars, ops[1]).max(rate_of(&local, ops[0])).max(ctrl[bi]);
                            if let Some(root) = root_of(&roots, ops[0]) {
                                self.raise_var(root, r);
                            }
                        }
                        Op::AccessChain | Op::InBoundsAccessChain | Op::PtrAccessChain | Op::CopyObject | Op::CopyLogical => {
                            let root = root_of(&roots, ops[0]);
                            roots.insert(inst.result_id.unwrap(), root);
                            result = Some(join(&local));
                        }
                        Op::Phi => {
                            let mut r = Rate::Const;
                            for pair in inst.operands.chunks(2) {
                                if let [rspirv::dr::Operand::IdRef(v), rspirv::dr::Operand::IdRef(pred)] = pair {
                                    r = r.max(rate_of(&local, *v));
                                    if let Some(&pb) = info.label_index.get(pred) {
                                        r = r.max(ctrl[pb]);
                                        if let Some(c) = cfg.cond[pb] {
                                            r = r.max(rate_of(&local, c));
                                        }
                                    }
                                }
                            }
                            result = Some(r);
                        }
                        Op::FunctionCall => {
                            let callee = *l.function_index.get(&ops[0]).ok_or_else(|| anyhow::anyhow!("call to unknown function %{}", ops[0]))?;
                            let arg_rates: Vec<Rate> = ops[1..].iter().map(|a| rate_of(&local, *a)).collect();
                            let arg_roots: Vec<Option<u32>> = ops[1..].iter().map(|a| root_of(&roots, *a)).collect();
                            result = Some(self.analyze_fn(callee, &arg_rates, &arg_roots)?);
                        }
                        Op::ReturnValue => {
                            ret = ret.max(rate_of(&local, ops[0]));
                        }
                        _ if is_image_op(op, inst.class.opname) || is_derivative(inst.class.opname) => {
                            result = Some(Rate::Pixel);
                        }
                        _ => {
                            result = Some(join(&local));
                        }
                    }
                    if let (Some(id), Some(r)) = (inst.result_id, result) {
                        let e = local.entry(id).or_insert(Rate::Const);
                        if r > *e {
                            *e = r;
                            local_changed = true;
                        }
                    }
                }
            }
            if !local_changed {
                break;
            }
        }
        for (id, r) in local {
            let e = self.ids.entry(id).or_insert(Rate::Const);
            if r > *e {
                *e = r;
                self.changed = true;
            }
        }
        self.stack.pop();
        Ok(ret)
    }
}

pub fn analyze(body: &Body<'_>) -> Result<Rates> {
    let l = body.lifted;
    let mut ctx = Ctx { body, ids: HashMap::new(), vars: HashMap::new(), changed: false, stack: Vec::new(), visited: vec![false; l.functions.len()] };
    let entry_fi = l.entry().ok().and_then(|e| l.function_index.get(&e.function).copied());
    if let Some(fi) = entry_fi {
        for _ in 0..1000 {
            ctx.changed = false;
            ctx.analyze_fn(fi, &[], &[])?;
            if !ctx.changed {
                break;
            }
        }
    }
    for fi in 0..l.functions.len() {
        if ctx.visited[fi] {
            continue;
        }
        let n = l.functions[fi].params.len();
        for _ in 0..1000 {
            ctx.changed = false;
            ctx.analyze_fn(fi, &vec![Rate::Pixel; n], &vec![None; n])?;
            if !ctx.changed {
                break;
            }
        }
    }
    Ok(Rates { ids: ctx.ids, vars: ctx.vars })
}
