//! CPU interpreter for the Fragment execution model.
//!
//! Pixels are evaluated in 2x2 quads aligned to even coordinates, the four invocations of a quad
//! advancing in lockstep at every derivative instruction (see [`exec`]). Rows of quads are
//! distributed over threads with rayon; the quad itself never splits.

pub mod dump;
pub mod exec;
pub mod ext;
pub mod image;
pub mod value;

use crate::lift::{ConstKind, Lifted, Type};
use crate::npy::Image;
use anyhow::{anyhow, bail, Context, Result};
use rayon::prelude::*;
use spirv::{BuiltIn, ExecutionModel, StorageClass};
use std::collections::BTreeMap;

pub use exec::{Invocation, Status};
pub use image::{BoundImage, Filter};
pub use value::{Mode, Ptr, Value};

/// A sampler binding from the command line: `name=path.npy[:nearest]`.
#[derive(Clone, Debug)]
pub struct SamplerSpec {
    pub name: String,
    pub image: Image,
    pub filter: Filter,
}

#[derive(Clone, Debug)]
pub struct EvalConfig {
    pub width: usize,
    pub height: usize,
    pub mode: Mode,
    pub samplers: Vec<SamplerSpec>,
    /// Uniform block members by name, scenario value syntax (`1.0`, `3`, `[1,2,3]`, 16 numbers
    /// column-major for a mat4).
    pub uniforms: Vec<(String, String)>,
    /// Extra per-pixel-constant `Location` inputs by variable name, same value syntax.
    pub inputs: Vec<(String, String)>,
    pub sampler_weight_bits: u32,
    /// Value written to every channel of a discarded pixel.
    pub discard_value: f32,
    /// Shader name used in error messages.
    pub label: String,
    /// Record per-result float ranges (`EvalOutput::ranges`).
    pub profile: bool,
    /// Evaluate every `stride`-th quad in each dimension (1 = every pixel); quads at
    /// `(2*i*stride, 2*j*stride)` still run whole so derivatives are exact.
    pub stride: usize,
    /// Result ids whose float value is rounded to f16 after it is computed (in any mode): a
    /// prediction of demoting that site to `RelaxedPrecision`/explicit f16.
    pub f16_sites: Vec<u32>,
    /// Round every float-typed result to f16 (`--f16-all`).
    pub f16_all: bool,
}

impl EvalConfig {
    pub fn new(width: usize, height: usize, mode: Mode) -> Self {
        EvalConfig {
            width,
            height,
            mode,
            samplers: Vec::new(),
            uniforms: Vec::new(),
            inputs: Vec::new(),
            sampler_weight_bits: 0,
            discard_value: f32::NAN,
            label: "<shader>".into(),
            profile: false,
            stride: 1,
            f16_sites: Vec::new(),
            f16_all: false,
        }
    }
}

/// Running min/max/NaN/inf statistics of one float-typed result id over the evaluated pixels.
/// `min`/`max` are over finite components only (vectors and matrices contribute every
/// component); `samples` counts evaluations of the instruction, not components.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RangeAcc {
    pub min: f64,
    pub max: f64,
    pub nan: u64,
    pub inf: u64,
    pub samples: u64,
}

impl RangeAcc {
    pub const EMPTY: RangeAcc = RangeAcc { min: f64::INFINITY, max: f64::NEG_INFINITY, nan: 0, inf: 0, samples: 0 };

    #[inline]
    pub fn record(&mut self, v: &Value) {
        self.samples += 1;
        self.record_leaves(v);
    }

    fn record_leaves(&mut self, v: &Value) {
        match v {
            Value::F(x) => {
                if x.is_nan() {
                    self.nan += 1;
                } else if x.is_infinite() {
                    self.inf += 1;
                } else {
                    if *x < self.min {
                        self.min = *x;
                    }
                    if *x > self.max {
                        self.max = *x;
                    }
                }
            }
            Value::V(xs) => {
                for x in xs {
                    self.record_leaves(x);
                }
            }
            _ => {}
        }
    }

    pub fn merge(&mut self, o: &RangeAcc) {
        self.min = self.min.min(o.min);
        self.max = self.max.max(o.max);
        self.nan += o.nan;
        self.inf += o.inf;
        self.samples += o.samples;
    }

    pub fn to_range(&self) -> crate::analysis::Range {
        crate::analysis::Range {
            min: if self.min.is_finite() { self.min } else { f64::NAN },
            max: if self.max.is_finite() { self.max } else { f64::NAN },
            nan: self.nan,
            inf: self.inf,
            samples: self.samples,
        }
    }
}

/// Rounds every float leaf of a value to f16 (RNE through f32) and widens it back.
pub fn round_f16(v: &mut Value) {
    match v {
        Value::F(x) => *x = value::r16(*x as f32),
        Value::V(xs) => xs.iter_mut().for_each(round_f16),
        _ => {}
    }
}

#[derive(Clone, Debug)]
pub struct EvalOutput {
    pub width: usize,
    pub height: usize,
    /// Output images by `Location`.
    pub outputs: BTreeMap<u32, Image>,
    pub discarded_pixels: usize,
    /// Derivative evaluations that had to substitute 0 because a neighbor in the quad had
    /// already been discarded.
    pub dead_derivatives: usize,
    /// Per float-typed result id: ranges over the evaluated pixels (`EvalConfig::profile`).
    pub ranges: Option<BTreeMap<u32, crate::analysis::Range>>,
}

#[derive(Clone, Debug)]
pub enum InputKind {
    /// `Location 0` vec2: `((x + 0.5) / W, (y + 0.5) / H)`.
    Uv,
    FragCoord,
    Const(Value),
}

#[derive(Clone, Debug)]
pub struct OutputVar {
    pub id: u32,
    pub location: u32,
}

/// Everything that is fixed for one evaluation: the lifted module, bindings and the initial
/// state of every invocation.
pub struct Program<'a> {
    pub lifted: &'a Lifted,
    pub label: String,
    pub mode: Mode,
    pub width: usize,
    pub height: usize,
    pub weight_bits: u32,
    pub images: Vec<BoundImage>,
    pub entry_fn: usize,
    /// SSA values that never change: constants and pointers to global variables.
    pub template_vals: Vec<Value>,
    /// Initial memory of every global variable (uniforms, samplers, private, zeroed I/O).
    pub template_mem: Vec<Value>,
    pub inputs: Vec<(u32, InputKind)>,
    pub outputs: Vec<OutputVar>,
    pub glsl_set: Option<u32>,
    /// By result id: the result type is a float scalar, vector or matrix.
    pub float_ids: Vec<bool>,
    /// By result id: round the result to f16 after computing it.
    pub f16_sites: Vec<bool>,
    pub profile: bool,
}

impl<'a> Program<'a> {
    pub fn new(lifted: &'a Lifted, cfg: &EvalConfig) -> Result<Self> {
        let entry = lifted.entry()?;
        if entry.model != ExecutionModel::Fragment {
            bail!("{}: entry point {:?} is {:?}, not Fragment", cfg.label, entry.name, entry.model);
        }
        let entry_fn = *lifted
            .function_index
            .get(&entry.function)
            .ok_or_else(|| anyhow!("{}: entry point function %{} not found", cfg.label, entry.function))?;
        let glsl_set = lifted.ext_inst_imports.iter().find(|(_, n)| n.as_str() == "GLSL.std.450").map(|(id, _)| *id);
        let mut prog = Program {
            lifted,
            label: cfg.label.clone(),
            mode: cfg.mode,
            width: cfg.width,
            height: cfg.height,
            weight_bits: cfg.sampler_weight_bits,
            images: Vec::new(),
            entry_fn,
            template_vals: vec![Value::Undef; lifted.bound as usize],
            template_mem: vec![Value::Undef; lifted.bound as usize],
            inputs: Vec::new(),
            outputs: Vec::new(),
            glsl_set,
            float_ids: vec![false; lifted.bound as usize],
            f16_sites: vec![false; lifted.bound as usize],
            profile: cfg.profile,
        };
        for (id, ty) in &lifted.result_types {
            if (*id as usize) < prog.float_ids.len() && crate::analysis::is_float_type(lifted, *ty) {
                prog.float_ids[*id as usize] = true;
                prog.f16_sites[*id as usize] = cfg.f16_all;
            }
        }
        for id in &cfg.f16_sites {
            if (*id as usize) >= prog.float_ids.len() {
                bail!("{}: --f16-sites {id}: result id out of range (bound {})", cfg.label, lifted.bound);
            }
            if !prog.float_ids[*id as usize] {
                bail!("{}: --f16-sites {id}: not a float-typed result id", cfg.label);
            }
            prog.f16_sites[*id as usize] = true;
        }
        for id in 0..lifted.bound {
            if lifted.constants.contains_key(&id) {
                let v = prog.const_value(id)?;
                prog.template_vals[id as usize] = v;
            }
        }
        let mut used_uniforms = vec![false; cfg.uniforms.len()];
        let mut used_samplers = vec![false; cfg.samplers.len()];
        let mut used_inputs = vec![false; cfg.inputs.len()];
        let mut uniform_members: Vec<String> = Vec::new();
        for var in &lifted.variables {
            let id = var.id;
            prog.template_vals[id as usize] = Value::Ptr(Ptr { root: id, path: Vec::new() });
            let name = var.name.clone().unwrap_or_else(|| format!("%{id}"));
            let init = match var.initializer {
                Some(c) => prog.const_value(c)?,
                None => Value::Undef,
            };
            let mem = match var.storage {
                StorageClass::Input => {
                    let kind = if let Some(b) = var.builtin {
                        match b {
                            BuiltIn::FragCoord => InputKind::FragCoord,
                            BuiltIn::FrontFacing => InputKind::Const(Value::Bool(true)),
                            BuiltIn::HelperInvocation => InputKind::Const(Value::Bool(false)),
                            BuiltIn::SampleId | BuiltIn::Layer | BuiltIn::ViewportIndex | BuiltIn::PrimitiveId => {
                                InputKind::Const(Value::I32(0))
                            }
                            other => bail!("{}: unsupported fragment input builtin {other:?} ({name})", cfg.label),
                        }
                    } else if var.location == Some(0) {
                        match lifted.ty(var.pointee)? {
                            Type::Vector { elem, count: 2 } if matches!(lifted.ty(*elem)?, Type::Float { .. }) => {}
                            _ => bail!(
                                "{}: Location 0 input {name} is {}, but the fullscreen triangle delivers a vec2 uv",
                                cfg.label,
                                lifted.type_name(var.pointee)
                            ),
                        }
                        InputKind::Uv
                    } else if let Some(i) = cfg.inputs.iter().position(|(n, _)| *n == name) {
                        used_inputs[i] = true;
                        InputKind::Const(parse_value(lifted, var.pointee, &cfg.inputs[i].1).with_context(|| {
                            format!("{}: --input {name}", cfg.label)
                        })?)
                    } else {
                        bail!(
                            "{}: input {name} ({}, Location {:?}) has no value; the runner only provides Location 0 uv; pass --input {name}=<value>",
                            cfg.label,
                            lifted.type_name(var.pointee),
                            var.location
                        );
                    };
                    prog.inputs.push((id, kind));
                    prog.zero(var.pointee)?
                }
                StorageClass::Output => {
                    if let Some(loc) = var.location {
                        prog.outputs.push(OutputVar { id, location: loc });
                    }
                    // Builtin outputs (FragDepth, SampleMask) are writable but ignored.
                    prog.zero(var.pointee)?
                }
                StorageClass::Uniform | StorageClass::PushConstant => {
                    let st = var.pointee;
                    let members = match lifted.ty(st)? {
                        Type::Struct { members } => members.clone(),
                        _ => bail!("{}: uniform variable {name} is not a block", cfg.label),
                    };
                    let mut vals = Vec::with_capacity(members.len());
                    let mut missing = Vec::new();
                    for (m, mty) in members.iter().enumerate() {
                        // Match by OpMemberName when present; `member<i>` and `offset<O>` are
                        // always accepted (glslang -g0 strips names).
                        let mname = lifted.member_name(st, m as u32).map(|s| s.to_string()).unwrap_or_else(|| format!("member{m}"));
                        let mkey = format!("member{m}");
                        let okey = lifted.member_decoration_u32(st, m as u32, spirv::Decoration::Offset).map(|o| format!("offset{o}"));
                        uniform_members.push(format!(
                            "{mname}: {} ({mkey}{})",
                            lifted.type_name(*mty),
                            okey.as_ref().map(|k| format!(", {k}")).unwrap_or_default()
                        ));
                        match cfg.uniforms.iter().position(|(n, _)| *n == mname || *n == mkey || Some(n) == okey.as_ref()) {
                            Some(i) => {
                                used_uniforms[i] = true;
                                vals.push(parse_value(lifted, *mty, &cfg.uniforms[i].1).with_context(|| {
                                    format!("{}: --uniform {mname}", cfg.label)
                                })?);
                            }
                            None if mname.starts_with("pad") => {
                                // Padding members are optional and zero-filled, matching the runner.
                                vals.push(parse_value(lifted, *mty, "0").unwrap_or(Value::Undef));
                            }
                            None => {
                                missing.push(mname);
                                vals.push(Value::Undef);
                            }
                        }
                    }
                    if !missing.is_empty() {
                        bail!(
                            "{}: uniform block {} is missing values for {}; members are: {}",
                            cfg.label,
                            lifted.name(st).unwrap_or(&name),
                            missing.join(", "),
                            uniform_members.join(", ")
                        );
                    }
                    Value::V(vals)
                }
                StorageClass::UniformConstant => match lifted.ty(var.pointee)? {
                    pt @ (Type::SampledImage { .. } | Type::Image { .. }) => {
                        let (is_sampled, image_ty) = match pt {
                            Type::SampledImage { image } => (true, *image),
                            _ => (false, var.pointee),
                        };
                        if let Type::Image { dim, arrayed, ms, .. } = lifted.ty(image_ty)? {
                            if *dim != spirv::Dim::Dim2D || *arrayed != 0 || *ms != 0 {
                                bail!("{}: sampler {name} is {}; only non-arrayed, single-sampled 2D images are supported", cfg.label, lifted.type_name(var.pointee));
                            }
                        }
                        // Match by OpName when present; `binding<B>` is always accepted
                        // (glslang -g0 strips names).
                        let bkey = var.binding.map(|b| format!("binding{b}"));
                        let i = cfg.samplers.iter().position(|s| s.name == name || Some(&s.name) == bkey.as_ref()).ok_or_else(|| {
                            anyhow!(
                                "{}: no --sampler for {name} ({}, {}); provide --sampler {}=path.npy[:nearest]",
                                cfg.label,
                                lifted.type_name(var.pointee),
                                bkey.as_deref().unwrap_or("no binding"),
                                if var.name.is_some() { name.clone() } else { bkey.clone().unwrap_or(name.clone()) }
                            )
                        })?;
                        used_samplers[i] = true;
                        let idx = prog.images.len();
                        prog.images.push(BoundImage {
                            name: name.clone(),
                            image: cfg.samplers[i].image.clone(),
                            filter: cfg.samplers[i].filter,
                        });
                        if is_sampled {
                            Value::SampledImage(idx)
                        } else {
                            Value::Image(idx)
                        }
                    }
                    Type::Sampler => Value::Sampler,
                    other => bail!("{}: unsupported UniformConstant variable {name} of type {other:?}", cfg.label),
                },
                StorageClass::Private => {
                    if init == Value::Undef {
                        prog.zero(var.pointee)?
                    } else {
                        init
                    }
                }
                other => bail!("{}: unsupported storage class {other:?} for global variable {name}", cfg.label),
            };
            prog.template_mem[id as usize] = mem;
        }
        for (i, (n, _)) in cfg.uniforms.iter().enumerate() {
            if !used_uniforms[i] {
                bail!(
                    "{}: --uniform {n} does not name a uniform block member; members are: {}",
                    cfg.label,
                    if uniform_members.is_empty() { "(none)".to_string() } else { uniform_members.join(", ") }
                );
            }
        }
        for (i, s) in cfg.samplers.iter().enumerate() {
            if !used_samplers[i] {
                let names: Vec<String> = lifted
                    .variables
                    .iter()
                    .filter(|v| v.storage == StorageClass::UniformConstant)
                    .map(|v| {
                        format!(
                            "{} ({})",
                            v.name.clone().unwrap_or_else(|| format!("%{}", v.id)),
                            v.binding.map(|b| format!("binding{b}")).unwrap_or("no binding".into())
                        )
                    })
                    .collect();
                bail!("{}: --sampler {} does not name a sampler variable; samplers are: {}", cfg.label, s.name, names.join(", "));
            }
        }
        for (i, (n, _)) in cfg.inputs.iter().enumerate() {
            if !used_inputs[i] {
                bail!("{}: --input {n} does not name an input variable", cfg.label);
            }
        }
        if prog.outputs.is_empty() {
            bail!("{}: the shader has no Output variable with a Location", cfg.label);
        }
        Ok(prog)
    }

    /// Zero value of a type (used for uninitialized variables, `OpConstantNull`, `OpUndef`).
    pub fn zero(&self, ty: u32) -> Result<Value> {
        let l = self.lifted;
        Ok(match l.ty(ty)? {
            Type::Void => Value::Undef,
            Type::Bool => Value::Bool(false),
            Type::Int { width: 32, signed } => value::mk_int(*signed, 0),
            Type::Int { width, .. } => bail!("{}: unsupported {width}-bit integer type", self.label),
            Type::Float { .. } => Value::F(0.0),
            Type::Vector { elem, count } => Value::V(vec![self.zero(*elem)?; *count as usize]),
            Type::Matrix { column, columns } => Value::V(vec![self.zero(*column)?; *columns as usize]),
            Type::Array { elem, len } => Value::V(vec![self.zero(*elem)?; l.array_len(*len)? as usize]),
            Type::RuntimeArray { .. } => bail!("{}: runtime arrays are not supported", self.label),
            Type::Struct { members } => Value::V(members.iter().map(|m| self.zero(*m)).collect::<Result<_>>()?),
            Type::Pointer { .. } => Value::Undef,
            Type::Image { .. } => Value::Undef,
            Type::Sampler => Value::Sampler,
            Type::SampledImage { .. } => Value::Undef,
            Type::Function { .. } => Value::Undef,
            Type::Other(op) => bail!("{}: unsupported type opcode {op:?}", self.label),
        })
    }

    pub fn const_value(&self, id: u32) -> Result<Value> {
        let l = self.lifted;
        let c = l.constants.get(&id).ok_or_else(|| anyhow!("%{id} is not a constant"))?;
        Ok(match &c.kind {
            ConstKind::Bool(b) => Value::Bool(*b),
            ConstKind::Bits32(bits) => match l.ty(c.ty)? {
                Type::Int { width: 32, signed } => value::mk_int(*signed, *bits),
                Type::Float { width: 32 } => Value::F(f32::from_bits(*bits) as f64),
                Type::Float { width: 16 } => Value::F(value::from_f16_bits(*bits as u16)),
                other => bail!("{}: unsupported constant type {other:?} for %{id}", self.label),
            },
            ConstKind::Bits64(bits) => match l.ty(c.ty)? {
                Type::Float { width: 64 } => Value::F(f64::from_bits(*bits)),
                other => bail!("{}: unsupported 64-bit constant type {other:?} for %{id}", self.label),
            },
            ConstKind::Composite(ids) => Value::V(ids.iter().map(|i| self.const_value(*i)).collect::<Result<_>>()?),
            ConstKind::Null | ConstKind::Undef => self.zero(c.ty)?,
            ConstKind::SpecOp(op) => bail!("{}: OpSpecConstantOp {op:?} (%{id}) is not supported", self.label),
        })
    }

    /// Scalar kind of a scalar/vector/matrix type.
    pub fn scalar_kind(&self, ty: u32) -> Result<Kind> {
        let l = self.lifted;
        Ok(match l.ty(ty)? {
            Type::Bool => Kind::Bool,
            Type::Int { width: 32, signed } => Kind::Int { signed: *signed },
            Type::Int { width, .. } => bail!("{}: unsupported {width}-bit integer type", self.label),
            Type::Float { width } => Kind::Float { width: *width },
            Type::Vector { elem, .. } => self.scalar_kind(*elem)?,
            Type::Matrix { column, .. } => self.scalar_kind(*column)?,
            other => bail!("{}: type {other:?} has no scalar kind", self.label),
        })
    }

    /// Float precision helper for a result type.
    pub fn fp(&self, ty: u32) -> Result<value::Fp> {
        match self.scalar_kind(ty)? {
            Kind::Float { width } => Ok(value::Fp { prec: self.mode.prec(width) }),
            other => bail!("{}: expected a float type, got {other:?}", self.label),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Bool,
    Int { signed: bool },
    Float { width: u32 },
}

/// Parses the scenario value syntax (`1.0`, `3`, `true`, `[1, 2, 3]`, 16 numbers for a mat4,
/// nested brackets allowed and ignored) into a value of type `ty`. Scalars are filled in
/// declaration order (matrices column-major). Floats are rounded to the type's storage width.
pub fn parse_value(lifted: &Lifted, ty: u32, text: &str) -> Result<Value> {
    let tokens: Vec<&str> = text
        .split(|c: char| c == ',' || c == '[' || c == ']' || c == '(' || c == ')' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .collect();
    let mut it = tokens.iter();
    let v = fill(lifted, ty, &mut it)?;
    let rest: Vec<&&str> = it.collect();
    if !rest.is_empty() {
        bail!("too many numbers for {}: {} left over", lifted.type_name(ty), rest.len());
    }
    Ok(v)
}

fn fill(lifted: &Lifted, ty: u32, it: &mut std::slice::Iter<&str>) -> Result<Value> {
    let mut next = || it.next().ok_or_else(|| anyhow!("not enough numbers for {}", lifted.type_name(ty)));
    Ok(match lifted.ty(ty)? {
        Type::Bool => {
            let t = next()?;
            Value::Bool(match *t {
                "true" | "1" => true,
                "false" | "0" => false,
                other => bail!("bad bool {other:?}"),
            })
        }
        Type::Int { width: 32, signed } => {
            let t = next()?;
            if *signed {
                Value::I32(t.parse::<i64>().map_err(|e| anyhow!("bad int {t:?}: {e}"))? as i32)
            } else {
                Value::U32(t.parse::<i64>().map_err(|e| anyhow!("bad uint {t:?}: {e}"))? as u32)
            }
        }
        Type::Int { width, .. } => bail!("unsupported {width}-bit integer uniform"),
        Type::Float { width } => {
            let t = next()?;
            let x: f64 = t.parse().map_err(|e| anyhow!("bad float {t:?}: {e}"))?;
            Value::F(match width {
                16 => value::r16(x as f32),
                32 => x as f32 as f64,
                _ => x,
            })
        }
        Type::Vector { elem, count } => Value::V((0..*count).map(|_| fill(lifted, *elem, it)).collect::<Result<_>>()?),
        Type::Matrix { column, columns } => Value::V((0..*columns).map(|_| fill(lifted, *column, it)).collect::<Result<_>>()?),
        Type::Array { elem, len } => {
            let n = lifted.array_len(*len)?;
            Value::V((0..n).map(|_| fill(lifted, *elem, it)).collect::<Result<_>>()?)
        }
        Type::Struct { members } => Value::V(members.iter().map(|m| fill(lifted, *m, it)).collect::<Result<_>>()?),
        other => bail!("cannot parse a value of type {other:?}"),
    })
}

/// Converts an output value to RGBA. Missing channels are 0 (a GPU leaves them undefined);
/// integer outputs are converted to float, matching the contract.
fn to_rgba(v: &Value) -> Result<[f32; 4]> {
    let mut out = [0.0f32; 4];
    for (i, c) in v.comps().iter().take(4).enumerate() {
        out[i] = match c {
            Value::F(x) => *x as f32,
            Value::I32(x) => *x as f32,
            Value::U32(x) => *x as f32,
            Value::Bool(b) => *b as u32 as f32,
            other => bail!("output component is {}", other.describe()),
        };
    }
    Ok(out)
}

/// One pixel's result.
pub(crate) enum PixelResult {
    Color(Vec<[f32; 4]>),
    Discarded,
}

/// Runs the four lanes of one quad to completion.
pub(crate) fn run_quad(lanes: &mut [Invocation<'_>; 4], x0: usize, y0: usize, dead_derivs: &mut usize) -> Result<[PixelResult; 4]> {
    let mut status: Vec<Status> = Vec::with_capacity(4);
    for (i, lane) in lanes.iter_mut().enumerate() {
        let (x, y) = (x0 + (i & 1), y0 + (i >> 1));
        lane.reset(x, y);
        status.push(lane.run()?);
    }
    loop {
        let pending: Vec<usize> = (0..4).filter(|&i| matches!(status[i], Status::Yield(_))).collect();
        if pending.is_empty() {
            break;
        }
        // All yielding lanes must be at the same derivative instruction.
        let first = match &status[pending[0]] {
            Status::Yield(r) => r.result,
            _ => unreachable!(),
        };
        for &i in &pending[1..] {
            if let Status::Yield(r) = &status[i] {
                if r.result != first {
                    bail!(
                        "{}: quad lanes diverged at derivative instructions %{} and %{}; control flow around derivatives must be uniform within a quad",
                        lanes[0].prog.label,
                        first,
                        r.result
                    );
                }
            }
        }
        let vals: Vec<Option<&Value>> = (0..4)
            .map(|i| match &status[i] {
                Status::Yield(r) => Some(&r.value),
                _ => None,
            })
            .collect();
        let req = match &status[pending[0]] {
            Status::Yield(r) => r.clone(),
            _ => unreachable!(),
        };
        let fp = lanes[0].prog.fp(req.ty)?;
        let mut results: Vec<(usize, Value)> = Vec::with_capacity(4);
        for &lane in &pending {
            results.push((lane, exec::derivative(&req, lane, &vals, fp, dead_derivs)?));
        }
        for (i, r) in results {
            status[i] = lanes[i].resume(req.result, r)?;
        }
    }
    let mut out: Vec<PixelResult> = Vec::with_capacity(4);
    for (i, lane) in lanes.iter().enumerate() {
        out.push(match status[i] {
            Status::Discarded => PixelResult::Discarded,
            Status::Finished => {
                let mut colors = Vec::with_capacity(lane.prog.outputs.len());
                for o in &lane.prog.outputs {
                    colors.push(to_rgba(&lane.mem[o.id as usize])?);
                }
                PixelResult::Color(colors)
            }
            Status::Yield(_) => unreachable!(),
        });
    }
    Ok(out.try_into().ok().unwrap())
}

/// Evaluates the fragment entry point at every pixel center.
pub fn evaluate(lifted: &Lifted, cfg: &EvalConfig) -> Result<EvalOutput> {
    let prog = Program::new(lifted, cfg)?;
    let (w, h) = (cfg.width, cfg.height);
    if w == 0 || h == 0 {
        bail!("{}: image size {w}x{h} is empty", cfg.label);
    }
    let n_out = prog.outputs.len();
    let quad_rows = (h + 1) / 2;
    let quad_cols = (w + 1) / 2;
    let stride = cfg.stride.max(1);
    struct RowPair {
        qy: usize,
        /// per output: 2 rows x w pixels
        colors: Vec<Vec<[f32; 4]>>,
        discarded: usize,
        dead_derivs: usize,
        profile: Option<Vec<RangeAcc>>,
    }
    let qys: Vec<usize> = (0..quad_rows).step_by(stride).collect();
    let rows: Vec<RowPair> = qys
        .into_par_iter()
        .map(|qy| -> Result<RowPair> {
            let mut lanes = [Invocation::new(&prog), Invocation::new(&prog), Invocation::new(&prog), Invocation::new(&prog)];
            let mut colors = vec![vec![[cfg.discard_value; 4]; 2 * w]; n_out];
            let mut discarded = 0;
            let mut dead_derivs = 0;
            let y0 = qy * 2;
            for qx in (0..quad_cols).step_by(stride) {
                let x0 = qx * 2;
                let res = run_quad(&mut lanes, x0, y0, &mut dead_derivs)
                    .with_context(|| format!("{}: at pixel ({x0}, {y0})", prog.label))?;
                for (i, r) in res.into_iter().enumerate() {
                    let (x, y) = (x0 + (i & 1), y0 + (i >> 1));
                    if x >= w || y >= h {
                        continue; // padding lane outside the image
                    }
                    match r {
                        PixelResult::Discarded => discarded += 1,
                        PixelResult::Color(cs) => {
                            for (o, c) in cs.into_iter().enumerate() {
                                colors[o][(y - y0) * w + x] = c;
                            }
                        }
                    }
                }
            }
            // Per-thread accumulators: the four lanes merge into one vector per quad row.
            let mut profile: Option<Vec<RangeAcc>> = None;
            for lane in lanes.iter_mut() {
                if let Some(p) = lane.profile.take() {
                    match &mut profile {
                        None => profile = Some(p),
                        Some(acc) => acc.iter_mut().zip(&p).for_each(|(a, b)| a.merge(b)),
                    }
                }
            }
            Ok(RowPair { qy, colors, discarded, dead_derivs, profile })
        })
        .collect::<Result<_>>()?;
    let mut outputs = BTreeMap::new();
    for (o, ov) in prog.outputs.iter().enumerate() {
        let mut img = Image::new(w, h);
        if stride > 1 {
            for y in 0..h {
                for x in 0..w {
                    img.set_texel(x, y, [cfg.discard_value; 4]);
                }
            }
        }
        for rp in rows.iter() {
            let qy = rp.qy;
            for dy in 0..2 {
                let y = qy * 2 + dy;
                if y >= h {
                    break;
                }
                for x in 0..w {
                    img.set_texel(x, y, rp.colors[o][dy * w + x]);
                }
            }
        }
        outputs.insert(ov.location, img);
    }
    let ranges = if cfg.profile {
        let mut acc = vec![RangeAcc::EMPTY; lifted.bound as usize];
        for rp in &rows {
            if let Some(p) = &rp.profile {
                acc.iter_mut().zip(p).for_each(|(a, b)| a.merge(b));
            }
        }
        Some(acc.iter().enumerate().filter(|(_, a)| a.samples > 0).map(|(id, a)| (id as u32, a.to_range())).collect())
    } else {
        None
    };
    Ok(EvalOutput {
        width: w,
        height: h,
        outputs,
        discarded_pixels: rows.iter().map(|r| r.discarded).sum(),
        dead_derivatives: rows.iter().map(|r| r.dead_derivs).sum(),
        ranges,
    })
}
