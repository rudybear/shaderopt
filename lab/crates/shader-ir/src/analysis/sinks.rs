//! Sink analysis: backward marking from the operands whose precision is structurally
//! load-bearing.
//!
//! Seeds:
//! * `address`: the coordinate (and any trailing image operand: bias, lod, gradients, offset) of
//!   every image sample/fetch/gather, every index of an `OpAccessChain`, the index of
//!   `OpVectorExtractDynamic`/`OpVectorInsertDynamic`.
//! * `control`: the condition of `OpBranchConditional`, the selector of `OpSwitch`, the
//!   condition of `OpSelect`.
//! * `discard`: the condition of a conditional branch (or switch) one of whose arms dominates a
//!   block containing `OpKill`/`OpTerminateInvocation`/`OpDemoteToHelperInvocation` or a call to
//!   a function that may kill.
//! * `convert`: the operand of `OpConvertFToS`/`OpConvertFToU`.
//!
//! Marks propagate to the id operands of pure instructions (arithmetic, `GLSL.std.450`,
//! comparisons, logic, conversions, composites, shuffles, selects, phis, derivatives, copies).
//! They stop at loads of `Uniform`/`PushConstant`/`Input`/`UniformConstant` variables and at
//! image instructions (the instruction itself is marked, its operands are not). A load of a
//! `Function`/`Private`/`Output` variable propagates to every value stored to that variable
//! (including through `OpCopyMemory` and pointer parameters), because that is where the loaded
//! value was computed. Function parameters propagate to the arguments at every call site, and
//! call results to the callee's `OpReturnValue` operands.

use super::{cfg::successors, Body};
use rspirv::dr::Operand;
use spirv::{Op, StorageClass};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sink {
    Address,
    Control,
    Discard,
    Convert,
}

impl Sink {
    pub fn as_str(self) -> &'static str {
        match self {
            Sink::Address => "address",
            Sink::Control => "control",
            Sink::Discard => "discard",
            Sink::Convert => "convert",
        }
    }
    fn bit(self) -> u8 {
        match self {
            Sink::Address => 1,
            Sink::Control => 2,
            Sink::Discard => 4,
            Sink::Convert => 8,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Marks(pub u8);

impl Marks {
    pub fn has(self, s: Sink) -> bool {
        self.0 & s.bit() != 0
    }
    pub fn any(self) -> bool {
        self.0 != 0
    }
}

fn is_image_inst(op: Op, name: &str) -> bool {
    !matches!(op, Op::Image | Op::SampledImage) && name.starts_with("Image")
}

fn is_sample_like(op: Op, name: &str) -> bool {
    matches!(op, Op::ImageFetch | Op::ImageRead | Op::ImageWrite)
        || name.starts_with("ImageSample")
        || name.starts_with("ImageGather")
        || name.starts_with("ImageDref")
        || name.starts_with("ImageSparse")
}

/// Functions that contain a kill (directly or through a call).
fn may_kill(body: &Body<'_>) -> Vec<bool> {
    let l = body.lifted;
    let n = l.functions.len();
    let mut mk = vec![false; n];
    for bi in &body.insts {
        if matches!(bi.inst.class.opcode, Op::Kill | Op::TerminateInvocation | Op::DemoteToHelperInvocation) {
            mk[bi.fi] = true;
        }
    }
    loop {
        let mut changed = false;
        for bi in &body.insts {
            if bi.inst.class.opcode == Op::FunctionCall && !mk[bi.fi] {
                if let Some(Operand::IdRef(c)) = bi.inst.operands.first() {
                    if l.function_index.get(c).map_or(false, |&ci| mk[ci]) {
                        mk[bi.fi] = true;
                        changed = true;
                    }
                }
            }
        }
        if !changed {
            break;
        }
    }
    mk
}

pub fn analyze(body: &Body<'_>) -> HashMap<u32, Marks> {
    let l = body.lifted;
    let mk = may_kill(body);
    // Kill blocks per function.
    let mut kill_blocks: Vec<Vec<usize>> = vec![Vec::new(); l.functions.len()];
    for bi in &body.insts {
        let op = bi.inst.class.opcode;
        let kills = matches!(op, Op::Kill | Op::TerminateInvocation | Op::DemoteToHelperInvocation)
            || (op == Op::FunctionCall
                && bi.inst.operands.first().and_then(|o| o.id_ref_any()).and_then(|c| l.function_index.get(&c)).map_or(false, |&ci| mk[ci]));
        if kills {
            if let Some(b) = bi.block {
                if !kill_blocks[bi.fi].contains(&b) {
                    kill_blocks[bi.fi].push(b);
                }
            }
        }
    }
    // Stores by root variable (a pointer parameter counts as a root), and parameter aliasing.
    let mut stores_by_root: HashMap<u32, Vec<u32>> = HashMap::new();
    let mut copies: Vec<(u32, u32)> = Vec::new(); // (dst root, src root)
    let mut param_to_roots: HashMap<u32, Vec<u32>> = HashMap::new();
    let mut root_to_params: HashMap<u32, Vec<u32>> = HashMap::new();
    let root_or_param = |id: u32| -> Option<u32> {
        let mut cur = id;
        for _ in 0..64 {
            if l.variables.iter().any(|v| v.id == cur) {
                return Some(cur);
            }
            let d = body.def(cur)?;
            match d.inst.class.opcode {
                Op::Variable | Op::FunctionParameter => return Some(cur),
                Op::AccessChain | Op::InBoundsAccessChain | Op::PtrAccessChain | Op::CopyObject | Op::CopyLogical => {
                    cur = d.inst.operands.first().and_then(|o| o.id_ref_any())?;
                }
                _ => return None,
            }
        }
        None
    };
    for bi in &body.insts {
        let inst = bi.inst;
        let ops = Body::id_operands(inst);
        match inst.class.opcode {
            Op::Store => {
                if let (Some(&p), Some(&v)) = (ops.first(), ops.get(1)) {
                    if let Some(r) = root_or_param(p) {
                        stores_by_root.entry(r).or_default().push(v);
                    }
                }
            }
            Op::CopyMemory => {
                if let (Some(&d), Some(&s)) = (ops.first(), ops.get(1)) {
                    if let (Some(dr), Some(sr)) = (root_or_param(d), root_or_param(s)) {
                        copies.push((dr, sr));
                    }
                }
            }
            Op::FunctionCall => {
                if let Some(&ci) = ops.first().and_then(|c| l.function_index.get(c)) {
                    for (pi, &a) in ops[1..].iter().enumerate() {
                        let Some(&p) = l.functions[ci].params.get(pi) else { continue };
                        if let Some(r) = root_or_param(a) {
                            param_to_roots.entry(p).or_default().push(r);
                            root_to_params.entry(r).or_default().push(p);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    // Every value that may have been written to the storage behind `root`.
    let var_sources = |root: u32| -> Vec<u32> {
        let mut out = Vec::new();
        let mut seen: HashSet<u32> = HashSet::new();
        let mut work = vec![root];
        while let Some(r) = work.pop() {
            if !seen.insert(r) {
                continue;
            }
            if let Some(v) = stores_by_root.get(&r) {
                out.extend(v.iter().copied());
            }
            for (d, s) in &copies {
                if *d == r {
                    work.push(*s);
                }
            }
            if let Some(ps) = root_to_params.get(&r) {
                work.extend(ps.iter().copied());
            }
            if let Some(rs) = param_to_roots.get(&r) {
                work.extend(rs.iter().copied());
            }
        }
        out
    };
    let is_local_storage = |root: u32| {
        matches!(
            body.storage_of_var(root),
            Some(StorageClass::Function) | Some(StorageClass::Private) | Some(StorageClass::Output) | Some(StorageClass::Workgroup)
        ) || body.op_of(root) == Some(Op::FunctionParameter)
    };

    // Seeds.
    let mut work: Vec<(u32, Sink)> = Vec::new();
    for bi in &body.insts {
        let inst = bi.inst;
        let op = inst.class.opcode;
        let name = inst.class.opname;
        if is_sample_like(op, name) {
            // Operand 0 is the image, 1 the coordinate, the rest image operands (bias, lod,
            // gradients, offset); for OpImageWrite operand 2 is the texel, not an address.
            let end = if op == Op::ImageWrite { 2 } else { inst.operands.len() };
            for o in &inst.operands[1.min(end)..end] {
                if let Some(id) = o.id_ref_any() {
                    work.push((id, Sink::Address));
                }
            }
        }
        match op {
            Op::AccessChain | Op::InBoundsAccessChain | Op::PtrAccessChain => {
                for o in inst.operands.iter().skip(1) {
                    if let Some(id) = o.id_ref_any() {
                        work.push((id, Sink::Address));
                    }
                }
            }
            Op::VectorExtractDynamic => {
                if let Some(id) = inst.operands.get(1).and_then(|o| o.id_ref_any()) {
                    work.push((id, Sink::Address));
                }
            }
            Op::VectorInsertDynamic => {
                if let Some(id) = inst.operands.get(2).and_then(|o| o.id_ref_any()) {
                    work.push((id, Sink::Address));
                }
            }
            Op::Select => {
                if let Some(id) = inst.operands.first().and_then(|o| o.id_ref_any()) {
                    work.push((id, Sink::Control));
                }
            }
            Op::BranchConditional | Op::Switch => {
                let Some(cond) = inst.operands.first().and_then(|o| o.id_ref_any()) else { continue };
                work.push((cond, Sink::Control));
                let cfg = &body.cfgs[bi.fi];
                let li = &l.functions[bi.fi].label_index;
                let arms = successors(inst);
                let targets: Vec<usize> = arms.iter().filter_map(|a| li.get(a).copied()).collect();
                if targets.iter().any(|&t| kill_blocks[bi.fi].iter().any(|&k| cfg.dominates(t, k))) {
                    work.push((cond, Sink::Discard));
                }
            }
            Op::ConvertFToS | Op::ConvertFToU => {
                if let Some(id) = inst.operands.first().and_then(|o| o.id_ref_any()) {
                    work.push((id, Sink::Convert));
                }
            }
            _ => {}
        }
    }

    // Propagation.
    let mut marks: HashMap<u32, Marks> = HashMap::new();
    while let Some((id, s)) = work.pop() {
        let m = marks.entry(id).or_default();
        if m.has(s) {
            continue;
        }
        m.0 |= s.bit();
        let Some(d) = body.def(id) else { continue };
        let inst = d.inst;
        let op = inst.class.opcode;
        let name = inst.class.opname;
        if is_image_inst(op, name) {
            continue;
        }
        match op {
            Op::Load => {
                let Some(ptr) = inst.operands.first().and_then(|o| o.id_ref_any()) else { continue };
                if let Some(root) = root_or_param(ptr) {
                    if is_local_storage(root) {
                        for v in var_sources(root) {
                            work.push((v, s));
                        }
                        // A pointer parameter (glslang passes `in` arguments as pointers to
                        // caller temporaries) is part of the chain: mark it too.
                        if body.op_of(root) == Some(Op::FunctionParameter) {
                            work.push((root, s));
                        }
                    }
                }
            }
            Op::Variable | Op::Label | Op::SampledImage | Op::Image | Op::AccessChain | Op::InBoundsAccessChain | Op::PtrAccessChain => {}
            Op::FunctionParameter => {
                for (_, a) in super::call_args_for_param(body, d.fi, id) {
                    work.push((a, s));
                }
            }
            Op::FunctionCall => {
                if let Some(&ci) = inst.operands.first().and_then(|o| o.id_ref_any()).and_then(|c| l.function_index.get(&c)) {
                    for bi in &body.insts {
                        if bi.fi == ci && bi.inst.class.opcode == Op::ReturnValue {
                            if let Some(v) = bi.inst.operands.first().and_then(|o| o.id_ref_any()) {
                                work.push((v, s));
                            }
                        }
                    }
                }
            }
            Op::Phi => {
                for pair in inst.operands.chunks(2) {
                    if let Some(Operand::IdRef(v)) = pair.first() {
                        work.push((*v, s));
                    }
                }
            }
            _ => {
                for o in Body::id_operands(inst) {
                    work.push((o, s));
                }
            }
        }
    }
    marks
}
