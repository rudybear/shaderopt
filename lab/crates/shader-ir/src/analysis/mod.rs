//! M2 static analysis: rates, sinks, sampler coordinate kinds, source lines, ranges.
//!
//! `analyze` walks every function body of a lifted module and produces one [`Entry`] per
//! instruction (parameters and labels included), the `samplers`, `outputs` and `summary` tables
//! of `lab/CONTRACTS.md` ("M2: analysis and rewrite CLIs"), and serializes them with
//! [`Analysis::to_json`].
//!
//! * [`rate`]: forward dataflow over the lattice `const < uniform < pixel` with a fixed point
//!   over Function/Private variables and phis, control dependence for divergent stores and phis,
//!   and per-call-site instantiation of callees.
//! * [`sinks`]: backward marking of `address`/`control`/`discard`/`convert` from their seeds
//!   through pure arithmetic and composite instructions.
//! * [`cfg`]: dominators, post-dominators and control dependence per function.
//! * [`lines`]: mapping to source lines through a `glslang -g` build of the same shader.

pub mod cfg;
pub mod lines;
pub mod rate;
pub mod sinks;

use crate::lift::{ConstKind, Lifted, Type};
use anyhow::{anyhow, bail, Result};
use rspirv::dr::{self, Operand};
use serde_json::{json, Map, Value as Json};
use spirv::{Op, StorageClass};
use std::collections::{BTreeMap, HashMap};

pub use rate::Rate;
pub use sinks::Sink;

/// Range statistics of one float-typed result id, as written by `eval --profile`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Range {
    pub min: f64,
    pub max: f64,
    pub nan: u64,
    pub inf: u64,
    pub samples: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyKind {
    Param,
    Label,
    Inst,
}

/// One instruction of a function body.
pub struct BodyInst<'a> {
    pub fi: usize,
    pub block: Option<usize>,
    /// Position in the function body, counting parameters, labels and every instruction.
    pub index: usize,
    pub inst: &'a dr::Instruction,
    pub kind: BodyKind,
}

/// Every function body of a module in layout order, plus per-function CFGs and an id index.
pub struct Body<'a> {
    pub lifted: &'a Lifted,
    pub insts: Vec<BodyInst<'a>>,
    /// Result id -> index into `insts`.
    pub by_id: HashMap<u32, usize>,
    /// (function index, block index) -> index into `insts` of the block's label.
    pub cfgs: Vec<cfg::Cfg>,
    /// Function index -> number of `OpFunctionCall` instructions targeting it.
    pub call_sites: Vec<usize>,
}

impl<'a> Body<'a> {
    pub fn new(lifted: &'a Lifted) -> Body<'a> {
        let mut insts = Vec::new();
        let mut by_id = HashMap::new();
        let mut cfgs = Vec::new();
        let mut call_sites = vec![0usize; lifted.functions.len()];
        for (fi, f) in lifted.module.functions.iter().enumerate() {
            let mut index = 0;
            let mut push = |inst: &'a dr::Instruction, block: Option<usize>, kind: BodyKind| {
                if let Some(id) = inst.result_id {
                    by_id.insert(id, insts.len());
                }
                insts.push(BodyInst { fi, block, index, inst, kind });
                index += 1;
            };
            for p in &f.parameters {
                push(p, None, BodyKind::Param);
            }
            for (bi, b) in f.blocks.iter().enumerate() {
                if let Some(l) = &b.label {
                    push(l, Some(bi), BodyKind::Label);
                }
                for inst in &b.instructions {
                    if inst.class.opcode == Op::FunctionCall {
                        if let Some(Operand::IdRef(callee)) = inst.operands.first() {
                            if let Some(&ci) = lifted.function_index.get(callee) {
                                call_sites[ci] += 1;
                            }
                        }
                    }
                    push(inst, Some(bi), BodyKind::Inst);
                }
            }
            cfgs.push(cfg::Cfg::build(f, &lifted.functions[fi].label_index));
        }
        Body { lifted, insts, by_id, cfgs, call_sites }
    }

    /// The defining body instruction of `id`, if it is defined in a function body.
    pub fn def(&self, id: u32) -> Option<&BodyInst<'a>> {
        self.by_id.get(&id).map(|&i| &self.insts[i])
    }

    pub fn op_of(&self, id: u32) -> Option<Op> {
        self.def(id).map(|d| d.inst.class.opcode)
    }

    /// Id operands of an instruction, skipping the set id of `OpExtInst`.
    pub fn id_operands(inst: &dr::Instruction) -> Vec<u32> {
        let skip = if inst.class.opcode == Op::ExtInst { 2 } else { 0 };
        inst.operands.iter().skip(skip).filter_map(|o| o.id_ref_any()).collect()
    }

    /// Root variable of a pointer id: `OpVariable` itself, the base of an access chain, or the
    /// operand of a copy. `None` for function parameters and anything else.
    pub fn root_of(&self, id: u32) -> Option<u32> {
        let mut cur = id;
        for _ in 0..64 {
            if self.lifted.variables.iter().any(|v| v.id == cur) {
                return Some(cur);
            }
            let d = self.def(cur)?;
            match d.inst.class.opcode {
                Op::Variable => return Some(cur),
                Op::AccessChain | Op::InBoundsAccessChain | Op::PtrAccessChain | Op::CopyObject | Op::CopyLogical => {
                    cur = d.inst.operands.first().and_then(|o| o.id_ref_any())?;
                }
                _ => return None,
            }
        }
        None
    }

    pub fn storage_of_var(&self, var: u32) -> Option<StorageClass> {
        if let Some(v) = self.lifted.variables.iter().find(|v| v.id == var) {
            return Some(v.storage);
        }
        let d = self.def(var)?;
        match d.inst.operands.first() {
            Some(Operand::StorageClass(s)) => Some(*s),
            _ => None,
        }
    }
}

/// Human-readable type spelling of the M2 contract: `f32`, `vec3<f32>`, `mat4<f32>`,
/// `ptr(Function, f32)`, `array<f32, 4>`, `struct Params`, `sampler2D`.
pub fn type_name(l: &Lifted, id: u32) -> String {
    match l.types.get(&id) {
        None => format!("%{id}"),
        Some(Type::Void) => "void".into(),
        Some(Type::Bool) => "bool".into(),
        Some(Type::Int { width, signed }) => format!("{}{width}", if *signed { "i" } else { "u" }),
        Some(Type::Float { width }) => format!("f{width}"),
        Some(Type::Vector { elem, count }) => format!("vec{count}<{}>", type_name(l, *elem)),
        Some(Type::Matrix { column, columns }) => match l.types.get(column) {
            Some(Type::Vector { elem, count }) if count == columns => format!("mat{columns}<{}>", type_name(l, *elem)),
            Some(Type::Vector { elem, count }) => format!("mat{columns}x{count}<{}>", type_name(l, *elem)),
            _ => format!("mat{columns}<?>"),
        },
        Some(Type::Array { elem, len }) => {
            format!("array<{}, {}>", type_name(l, *elem), l.array_len(*len).map(|n| n.to_string()).unwrap_or("?".into()))
        }
        Some(Type::RuntimeArray { elem }) => format!("array<{}>", type_name(l, *elem)),
        Some(Type::Struct { .. }) => format!("struct {}", l.name(id).map(|s| s.to_string()).unwrap_or_else(|| format!("%{id}"))),
        Some(Type::Pointer { storage, pointee }) => format!("ptr({storage:?}, {})", type_name(l, *pointee)),
        Some(Type::Image { dim, .. }) => format!("image{}", dim_name(*dim)),
        Some(Type::Sampler) => "sampler".into(),
        Some(Type::SampledImage { image }) => match l.types.get(image) {
            Some(Type::Image { dim, arrayed, .. }) => format!("sampler{}{}", dim_name(*dim), if *arrayed != 0 { "Array" } else { "" }),
            _ => "sampler?".into(),
        },
        Some(Type::Function { ret, params }) => {
            format!("fn({}) -> {}", params.iter().map(|p| type_name(l, *p)).collect::<Vec<_>>().join(", "), type_name(l, *ret))
        }
        Some(Type::Other(op)) => format!("{op:?}"),
    }
}

fn dim_name(d: spirv::Dim) -> &'static str {
    match d {
        spirv::Dim::Dim1D => "1D",
        spirv::Dim::Dim2D => "2D",
        spirv::Dim::Dim3D => "3D",
        spirv::Dim::DimCube => "Cube",
        spirv::Dim::DimRect => "Rect",
        spirv::Dim::DimBuffer => "Buffer",
        spirv::Dim::DimSubpassData => "SubpassData",
        _ => "?",
    }
}

/// Scalar float / float vector / float matrix.
pub fn is_float_type(l: &Lifted, ty: u32) -> bool {
    match l.types.get(&ty) {
        Some(Type::Float { .. }) => true,
        Some(Type::Vector { elem, .. }) => matches!(l.types.get(elem), Some(Type::Float { .. })),
        Some(Type::Matrix { column, .. }) => is_float_type(l, *column),
        _ => false,
    }
}

/// One instruction of the analysis output.
#[derive(Clone, Debug)]
pub struct Entry {
    pub id: u32,
    pub op: String,
    pub ext: Option<String>,
    pub ty: Option<String>,
    pub func: String,
    pub block: Option<usize>,
    pub index: usize,
    pub line: Option<u32>,
    pub name: Option<String>,
    pub operands: Vec<u32>,
    pub rate: Rate,
    pub sinks: Vec<Sink>,
    pub range: Option<Range>,
    /// Has a result id and a non-pointer, non-void result type.
    pub is_site: bool,
    pub is_float: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoordKind {
    UvExact,
    UvOffset,
    Other,
}

impl CoordKind {
    pub fn as_str(self) -> &'static str {
        match self {
            CoordKind::UvExact => "uv_exact",
            CoordKind::UvOffset => "uv_offset",
            CoordKind::Other => "other",
        }
    }
}

#[derive(Clone, Debug)]
pub struct SampleSite {
    pub id: u32,
    pub coord_id: u32,
    pub coord_kind: CoordKind,
    pub offset: Option<[f64; 2]>,
}

#[derive(Clone, Debug)]
pub struct SamplerInfo {
    pub name: String,
    pub id: u32,
    pub binding: Option<u32>,
    pub samples: Vec<SampleSite>,
}

#[derive(Clone, Debug)]
pub struct OutputInfo {
    pub location: u32,
    pub id: u32,
    pub ty: String,
}

#[derive(Clone, Debug)]
pub struct FunctionSummary {
    pub name: String,
    pub id: u32,
    pub call_sites: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Summary {
    pub pixel: usize,
    pub uniform: usize,
    pub const_: usize,
    pub sink_sites: usize,
    pub float_sites: usize,
    pub candidate_sites: usize,
}

#[derive(Clone, Debug)]
pub struct Analysis {
    pub shader: String,
    pub entry: String,
    pub bound: u32,
    pub instructions: Vec<Entry>,
    pub samplers: Vec<SamplerInfo>,
    pub outputs: Vec<OutputInfo>,
    pub functions: Vec<FunctionSummary>,
    pub summary: Summary,
}

pub struct AnalyzeOptions<'a> {
    pub shader: String,
    pub debug: Option<&'a Lifted>,
    pub ranges: Option<&'a BTreeMap<u32, Range>>,
}

/// Runs every analysis on `lifted`.
pub fn analyze(lifted: &Lifted, opts: &AnalyzeOptions<'_>) -> Result<Analysis> {
    let body = Body::new(lifted);
    let entry = lifted.entry()?;
    let rates = rate::analyze(&body)?;
    let sink_marks = sinks::analyze(&body);
    let lines: Vec<Option<u32>> = match opts.debug {
        Some(dbg) => lines::map_lines(&body, dbg)?,
        None => vec![None; body.insts.len()],
    };
    let mut instructions = Vec::with_capacity(body.insts.len());
    for (k, bi) in body.insts.iter().enumerate() {
        let inst = bi.inst;
        let id = inst.result_id.unwrap_or(0);
        let ext = if inst.class.opcode == Op::ExtInst { Some(ext_name(lifted, inst)) } else { None };
        let ty = inst.result_type.map(|t| type_name(lifted, t));
        let is_site = inst.result_id.is_some()
            && inst.result_type.map_or(false, |t| !matches!(lifted.types.get(&t), Some(Type::Pointer { .. }) | Some(Type::Void) | None));
        let is_float = inst.result_type.map_or(false, |t| is_float_type(lifted, t));
        let mut sinks_v: Vec<Sink> = Vec::new();
        if let Some(m) = sink_marks.get(&id).filter(|_| id != 0) {
            for s in [Sink::Address, Sink::Control, Sink::Discard, Sink::Convert] {
                if m.has(s) {
                    sinks_v.push(s);
                }
            }
        }
        let rate = rates.inst_rate(k, &body);
        instructions.push(Entry {
            id,
            op: format!("Op{}", inst.class.opname),
            ext,
            ty,
            func: lifted.functions[bi.fi].name.clone().unwrap_or_else(|| format!("%{}", lifted.functions[bi.fi].id)),
            block: bi.block,
            index: bi.index,
            line: lines[k],
            name: if id != 0 { lifted.name(id).map(|s| s.to_string()) } else { None },
            operands: Body::id_operands(inst),
            rate,
            sinks: sinks_v,
            range: opts.ranges.and_then(|r| r.get(&id).copied()).filter(|_| id != 0),
            is_site,
            is_float,
        });
    }
    let samplers = sampler_sites(&body, &rates);
    let outputs = lifted
        .variables
        .iter()
        .filter(|v| v.storage == StorageClass::Output && v.location.is_some())
        .map(|v| OutputInfo { location: v.location.unwrap(), id: v.id, ty: type_name(lifted, v.pointee) })
        .collect();
    let functions = lifted
        .functions
        .iter()
        .enumerate()
        .map(|(fi, f)| FunctionSummary { name: f.name.clone().unwrap_or_else(|| format!("%{}", f.id)), id: f.id, call_sites: body.call_sites[fi] })
        .collect();
    let mut summary = Summary::default();
    for e in &instructions {
        if !e.is_site {
            continue;
        }
        match e.rate {
            Rate::Pixel => summary.pixel += 1,
            Rate::Uniform => summary.uniform += 1,
            Rate::Const => summary.const_ += 1,
        }
        if !e.sinks.is_empty() {
            summary.sink_sites += 1;
        }
        if e.is_float {
            summary.float_sites += 1;
            if e.sinks.is_empty() {
                summary.candidate_sites += 1;
            }
        }
    }
    Ok(Analysis { shader: opts.shader.clone(), entry: entry.name.clone(), bound: lifted.bound, instructions, samplers, outputs, functions, summary })
}

fn ext_name(l: &Lifted, inst: &dr::Instruction) -> String {
    let set = inst.operands.first().and_then(|o| o.id_ref_any()).unwrap_or(0);
    let n = match inst.operands.get(1) {
        Some(Operand::LiteralExtInstInteger(n)) => *n,
        _ => 0,
    };
    let set_name = l.ext_inst_imports.get(&set).map(|s| s.as_str()).unwrap_or("?");
    if set_name == "GLSL.std.450" {
        match spirv::GLOp::from_u32(n) {
            Some(g) => format!("{g:?}"),
            None => format!("GLSL.std.450#{n}"),
        }
    } else {
        format!("{set_name}#{n}")
    }
}

// ---- samplers ------------------------------------------------------------------------------

fn is_sample_op(op: Op, name: &str) -> bool {
    matches!(op, Op::ImageFetch | Op::ImageRead)
        || name.starts_with("ImageSample")
        || name.starts_with("ImageGather")
        || name.starts_with("ImageDref")
        || name.starts_with("ImageSparseSample")
        || name.starts_with("ImageSparseFetch")
        || name.starts_with("ImageSparseGather")
}

/// Root sampler variables an image operand may come from (several through function parameters).
fn image_roots(body: &Body<'_>, id: u32, depth: usize) -> Vec<u32> {
    if depth > 32 {
        return Vec::new();
    }
    let Some(d) = body.def(id) else { return Vec::new() };
    match d.inst.class.opcode {
        Op::SampledImage | Op::Image | Op::CopyObject => {
            d.inst.operands.first().and_then(|o| o.id_ref_any()).map(|x| image_roots(body, x, depth + 1)).unwrap_or_default()
        }
        Op::Load => d.inst.operands.first().and_then(|o| o.id_ref_any()).and_then(|p| body.root_of(p)).into_iter().collect(),
        Op::FunctionParameter => {
            let mut out = Vec::new();
            for (pi, arg) in call_args_for_param(body, d.fi, id) {
                let _ = pi;
                for r in image_roots(body, arg, depth + 1) {
                    if !out.contains(&r) {
                        out.push(r);
                    }
                }
            }
            out
        }
        _ => Vec::new(),
    }
}

/// `(call instruction index, argument id)` for every call site's argument bound to `param`.
pub fn call_args_for_param(body: &Body<'_>, fi: usize, param: u32) -> Vec<(usize, u32)> {
    let Some(pi) = body.lifted.functions[fi].params.iter().position(|&p| p == param) else { return Vec::new() };
    let fid = body.lifted.functions[fi].id;
    let mut out = Vec::new();
    for (k, bi) in body.insts.iter().enumerate() {
        if bi.inst.class.opcode == Op::FunctionCall && bi.inst.operands.first().and_then(|o| o.id_ref_any()) == Some(fid) {
            if let Some(Operand::IdRef(a)) = bi.inst.operands.get(1 + pi) {
                out.push((k, *a));
            }
        }
    }
    out
}

fn sampler_sites(body: &Body<'_>, rates: &rate::Rates) -> Vec<SamplerInfo> {
    let l = body.lifted;
    let uv_var = l.variables.iter().find(|v| v.storage == StorageClass::Input && v.location == Some(0)).map(|v| v.id);
    let mut out = Vec::new();
    for v in &l.variables {
        if v.storage != StorageClass::UniformConstant || !matches!(l.types.get(&v.pointee), Some(Type::SampledImage { .. }) | Some(Type::Image { .. })) {
            continue;
        }
        let mut samples = Vec::new();
        for bi in &body.insts {
            let inst = bi.inst;
            if !is_sample_op(inst.class.opcode, inst.class.opname) {
                continue;
            }
            let Some(img) = inst.operands.first().and_then(|o| o.id_ref_any()) else { continue };
            if !image_roots(body, img, 0).contains(&v.id) {
                continue;
            }
            let Some(coord) = inst.operands.get(1).and_then(|o| o.id_ref_any()) else { continue };
            let (coord_kind, offset) = coord_kind(body, rates, uv_var, coord);
            samples.push(SampleSite { id: inst.result_id.unwrap_or(0), coord_id: coord, coord_kind, offset });
        }
        out.push(SamplerInfo { name: v.name.clone().unwrap_or_else(|| format!("%{}", v.id)), id: v.id, binding: v.binding, samples });
    }
    out
}

/// Looks through a load of a Function/Private variable that is written by exactly one direct
/// store (and never through an access chain or a call), returning the stored value.
fn resolve_single_store(body: &Body<'_>, id: u32) -> u32 {
    let mut cur = id;
    for _ in 0..16 {
        let Some(d) = body.def(cur) else { return cur };
        if d.inst.class.opcode != Op::Load {
            return cur;
        }
        let Some(ptr) = d.inst.operands.first().and_then(|o| o.id_ref_any()) else { return cur };
        if body.op_of(ptr) != Some(Op::Variable) {
            return cur;
        }
        // Every write to the variable must be a direct OpStore; collect them.
        let mut stored = Vec::new();
        for bi in &body.insts {
            let inst = bi.inst;
            match inst.class.opcode {
                Op::Store => {
                    let p = inst.operands.first().and_then(|o| o.id_ref_any());
                    if p == Some(ptr) {
                        stored.push(inst.operands.get(1).and_then(|o| o.id_ref_any()).unwrap_or(0));
                    } else if p.and_then(|p| body.root_of(p)) == Some(ptr) {
                        return cur;
                    }
                }
                Op::CopyMemory | Op::FunctionCall => {
                    if Body::id_operands(inst).iter().any(|&o| body.root_of(o) == Some(ptr)) {
                        return cur;
                    }
                }
                _ => {}
            }
        }
        if stored.len() != 1 {
            return cur;
        }
        cur = stored[0];
    }
    cur
}

fn is_uv_exact(body: &Body<'_>, uv_var: Option<u32>, id: u32, depth: usize) -> bool {
    let Some(uv) = uv_var else { return false };
    if depth > 16 {
        return false;
    }
    let id = resolve_single_store(body, id);
    let Some(d) = body.def(id) else { return false };
    let ops = Body::id_operands(d.inst);
    match d.inst.class.opcode {
        Op::Load => ops.first() == Some(&uv),
        Op::CopyObject | Op::CopyLogical => ops.first().map_or(false, |&o| is_uv_exact(body, uv_var, o, depth + 1)),
        Op::CompositeConstruct => {
            if ops.len() == 1 {
                return is_uv_exact(body, uv_var, ops[0], depth + 1);
            }
            if ops.len() != 2 {
                return false;
            }
            ops.iter().enumerate().all(|(k, &o)| {
                let o = resolve_single_store(body, o);
                match body.def(o) {
                    Some(e) if e.inst.class.opcode == Op::CompositeExtract => {
                        let src = e.inst.operands.first().and_then(|x| x.id_ref_any());
                        let idx = match e.inst.operands.get(1) {
                            Some(Operand::LiteralBit32(i)) => *i as usize,
                            _ => usize::MAX,
                        };
                        e.inst.operands.len() == 2 && idx == k && src.map_or(false, |s| is_uv_exact(body, uv_var, s, depth + 1))
                    }
                    _ => false,
                }
            })
        }
        Op::VectorShuffle => {
            let lits: Vec<u32> = d
                .inst
                .operands
                .iter()
                .skip(2)
                .filter_map(|o| match o {
                    Operand::LiteralBit32(v) => Some(*v),
                    _ => None,
                })
                .collect();
            lits == [0, 1] && ops.first().map_or(false, |&a| is_uv_exact(body, uv_var, a, depth + 1))
        }
        _ => false,
    }
}

/// Evaluates a small constant expression to its float components (constants, composites of
/// constants, negation, +, -, *, / of constants, int-to-float conversions, extracts).
pub fn const_floats(body: &Body<'_>, id: u32, depth: usize) -> Option<Vec<f64>> {
    let l = body.lifted;
    if depth > 16 {
        return None;
    }
    if let Some(c) = l.constants.get(&id) {
        return match &c.kind {
            ConstKind::Bits32(b) => match l.types.get(&c.ty) {
                Some(Type::Float { width: 32 }) => Some(vec![f32::from_bits(*b) as f64]),
                Some(Type::Float { width: 16 }) => Some(vec![half::f16::from_bits(*b as u16).to_f64()]),
                Some(Type::Int { signed: true, .. }) => Some(vec![*b as i32 as f64]),
                Some(Type::Int { signed: false, .. }) => Some(vec![*b as f64]),
                _ => None,
            },
            ConstKind::Bits64(b) => match l.types.get(&c.ty) {
                Some(Type::Float { width: 64 }) => Some(vec![f64::from_bits(*b)]),
                _ => None,
            },
            ConstKind::Composite(ids) => {
                let mut out = Vec::new();
                for i in ids {
                    out.extend(const_floats(body, *i, depth + 1)?);
                }
                Some(out)
            }
            ConstKind::Null => Some(vec![0.0; comp_count(l, c.ty)]),
            _ => None,
        };
    }
    let id = resolve_single_store(body, id);
    let d = body.def(id)?;
    let ops = Body::id_operands(d.inst);
    let bin = |f: fn(f64, f64) -> f64| -> Option<Vec<f64>> {
        let a = const_floats(body, *ops.first()?, depth + 1)?;
        let b = const_floats(body, *ops.get(1)?, depth + 1)?;
        let n = a.len().max(b.len());
        if a.len() != n && a.len() != 1 || b.len() != n && b.len() != 1 {
            return None;
        }
        Some((0..n).map(|i| f(a[i.min(a.len() - 1)], b[i.min(b.len() - 1)])).collect())
    };
    match d.inst.class.opcode {
        Op::CompositeConstruct => {
            let mut out = Vec::new();
            for o in ops {
                out.extend(const_floats(body, o, depth + 1)?);
            }
            Some(out)
        }
        Op::FNegate => Some(const_floats(body, *ops.first()?, depth + 1)?.into_iter().map(|x| -x).collect()),
        Op::FAdd => bin(|a, b| a + b),
        Op::FSub => bin(|a, b| a - b),
        Op::FMul | Op::VectorTimesScalar => bin(|a, b| a * b),
        Op::FDiv => bin(|a, b| a / b),
        Op::ConvertSToF | Op::ConvertUToF | Op::FConvert | Op::CopyObject => const_floats(body, *ops.first()?, depth + 1),
        Op::CompositeExtract => {
            let v = const_floats(body, *ops.first()?, depth + 1)?;
            let idx = match d.inst.operands.get(1) {
                Some(Operand::LiteralBit32(i)) => *i as usize,
                _ => return None,
            };
            (d.inst.operands.len() == 2).then(|| v.get(idx).copied()).flatten().map(|x| vec![x])
        }
        Op::VectorShuffle => {
            let a = const_floats(body, *ops.first()?, depth + 1)?;
            let b = const_floats(body, *ops.get(1)?, depth + 1)?;
            let mut out = Vec::new();
            for o in d.inst.operands.iter().skip(2) {
                let i = match o {
                    Operand::LiteralBit32(i) => *i as usize,
                    _ => return None,
                };
                out.push(if i < a.len() { a[i] } else { *b.get(i - a.len())? });
            }
            Some(out)
        }
        _ => None,
    }
}

fn comp_count(l: &Lifted, ty: u32) -> usize {
    match l.types.get(&ty) {
        Some(Type::Vector { count, .. }) => *count as usize,
        _ => 1,
    }
}

fn coord_kind(body: &Body<'_>, rates: &rate::Rates, uv_var: Option<u32>, coord: u32) -> (CoordKind, Option<[f64; 2]>) {
    if is_uv_exact(body, uv_var, coord, 0) {
        return (CoordKind::UvExact, None);
    }
    let id = resolve_single_store(body, coord);
    if let Some(d) = body.def(id) {
        let ops = Body::id_operands(d.inst);
        let op = d.inst.class.opcode;
        if matches!(op, Op::FAdd | Op::FSub) && ops.len() == 2 {
            let (a, b) = (ops[0], ops[1]);
            let per_dispatch = |x: u32| rates.rate_of_id(x) <= Rate::Uniform;
            let as_offset = |x: u32, neg: bool| -> Option<[f64; 2]> {
                let v = const_floats(body, x, 0)?;
                let s = if neg { -1.0 } else { 1.0 };
                match v.as_slice() {
                    [x, y] => Some([s * x, s * y]),
                    [x] => Some([s * x, s * x]),
                    _ => None,
                }
            };
            if is_uv_exact(body, uv_var, a, 0) && per_dispatch(b) {
                return (CoordKind::UvOffset, as_offset(b, op == Op::FSub));
            }
            if op == Op::FAdd && is_uv_exact(body, uv_var, b, 0) && per_dispatch(a) {
                return (CoordKind::UvOffset, as_offset(a, false));
            }
        }
    }
    (CoordKind::Other, None)
}

// ---- JSON ----------------------------------------------------------------------------------

fn f64_json(x: f64) -> Json {
    if x.is_finite() {
        json!(x)
    } else {
        Json::Null
    }
}

impl Analysis {
    pub fn to_json(&self) -> Json {
        let insts: Vec<Json> = self
            .instructions
            .iter()
            .map(|e| {
                let mut m = Map::new();
                m.insert("id".into(), json!(e.id));
                m.insert("op".into(), json!(e.op));
                m.insert("ext".into(), json!(e.ext));
                m.insert("type".into(), json!(e.ty));
                m.insert("func".into(), json!(e.func));
                m.insert("block".into(), json!(e.block));
                m.insert("index".into(), json!(e.index));
                m.insert("line".into(), json!(e.line));
                m.insert("name".into(), json!(e.name));
                m.insert("rate".into(), json!(e.rate.as_str()));
                m.insert("sinks".into(), json!(e.sinks.iter().map(|s| s.as_str()).collect::<Vec<_>>()));
                m.insert("operands".into(), json!(e.operands));
                m.insert(
                    "range".into(),
                    match e.range {
                        Some(r) => json!({"min": f64_json(r.min), "max": f64_json(r.max), "nan": r.nan, "inf": r.inf, "samples": r.samples}),
                        None => Json::Null,
                    },
                );
                Json::Object(m)
            })
            .collect();
        json!({
            "shader": self.shader,
            "entry": self.entry,
            "bound": self.bound,
            "instructions": insts,
            "samplers": self.samplers.iter().map(|s| json!({
                "name": s.name, "id": s.id, "binding": s.binding,
                "samples": s.samples.iter().map(|x| json!({
                    "id": x.id, "coord_id": x.coord_id, "coord_kind": x.coord_kind.as_str(),
                    "offset": x.offset.map(|o| json!([o[0], o[1]])).unwrap_or(Json::Null),
                })).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
            "outputs": self.outputs.iter().map(|o| json!({"location": o.location, "id": o.id, "type": o.ty})).collect::<Vec<_>>(),
            "functions": self.functions.iter().map(|f| json!({"name": f.name, "id": f.id, "call_sites": f.call_sites})).collect::<Vec<_>>(),
            "summary": {
                "pixel": self.summary.pixel, "uniform": self.summary.uniform, "const": self.summary.const_,
                "sink_sites": self.summary.sink_sites, "float_sites": self.summary.float_sites,
                "candidate_sites": self.summary.candidate_sites,
            },
        })
    }
}

/// Parses a `ranges.json` written by `eval --profile`.
pub fn parse_ranges(text: &str) -> Result<BTreeMap<u32, Range>> {
    let v: Json = serde_json::from_str(text).map_err(|e| anyhow!("ranges.json: {e}"))?;
    let obj = v.as_object().ok_or_else(|| anyhow!("ranges.json: expected an object keyed by result id"))?;
    let mut out = BTreeMap::new();
    for (k, r) in obj {
        let id: u32 = k.parse().map_err(|_| anyhow!("ranges.json: key {k:?} is not a result id"))?;
        let num = |f: &str| r.get(f).and_then(|x| x.as_f64());
        let int = |f: &str| r.get(f).and_then(|x| x.as_u64()).unwrap_or(0);
        if r.get("samples").is_none() {
            bail!("ranges.json: entry {k} has no samples field");
        }
        out.insert(
            id,
            Range { min: num("min").unwrap_or(f64::NAN), max: num("max").unwrap_or(f64::NAN), nan: int("nan"), inf: int("inf"), samples: int("samples") },
        );
    }
    Ok(out)
}

/// Serializes ranges as `eval --profile` writes them.
pub fn ranges_json(ranges: &BTreeMap<u32, Range>) -> Json {
    let mut m = Map::new();
    for (id, r) in ranges {
        m.insert(id.to_string(), json!({"min": f64_json(r.min), "max": f64_json(r.max), "nan": r.nan, "inf": r.inf, "samples": r.samples}));
    }
    Json::Object(m)
}
