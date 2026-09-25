//! M2 exact rewrite passes (`lab/CONTRACTS.md`, "M2: analysis and rewrite CLIs", `rewrite`).
//!
//! Every pass mutates `Lifted::module` in place (the IR *is* the `rspirv::dr::Module`) and calls
//! [`Lifted::reanalyze`] after its edits. Untouched instructions keep their result ids; new
//! results take fresh ids above the old bound (the header bound is updated). Each change is
//! recorded as an [`EditOp`], the `ops.json` record of the contract, with `class` `exact`
//! (bit-identical on every input by IEEE semantics) or `ulp` (identical in real arithmetic, a
//! bounded rounding difference).
//!
//! Passes: [`fold`], [`dce`], [`cse`], [`ident`], [`unroll`], [`divconst`], [`powspec`],
//! [`select`]; [`run_pipeline`] applies a list of them and repeats the cleanup passes
//! `fold,dce,cse,ident` to a fixed point after every other pass. [`validate`] runs the
//! spirv-tools validator in-process. The M3 precision demotion ([`demote`], `shader-ir demote`)
//! is a separate transform with its own class `lossy` (the value changes by design).

pub mod cfg;
pub mod consts;
pub mod cse;
pub mod dce;
pub mod demote;
pub mod divconst;
pub mod fold;
pub mod ident;
pub mod powspec;
pub mod select;
pub mod unroll;

use crate::lift::{Lifted, Site, Type};
use anyhow::{anyhow, bail, Result};
use rspirv::dr::{self, Operand};
use spirv::{Op, StorageClass};
use std::collections::{HashMap, HashSet};

/// The correctness class of an edit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    /// Bit-identical on every input by IEEE semantics.
    Exact,
    /// Identical in real arithmetic; bounded rounding difference.
    Ulp,
    /// Changes the value by design (M3 precision demotion: `RelaxedPrecision`, f16); the
    /// caller bounds the error with the interpreter's `--f16-sites` prediction and the GPU A/B.
    Lossy,
}

impl Class {
    pub fn as_str(self) -> &'static str {
        match self {
            Class::Exact => "exact",
            Class::Ulp => "ulp",
            Class::Lossy => "lossy",
        }
    }
}

/// One record of `ops.json`: `{"pass", "class", "target", "replaced_by", "detail"}`.
#[derive(Clone, Debug, PartialEq)]
pub struct EditOp {
    pub pass: &'static str,
    pub class: Class,
    /// The id the edit targets (a result id, or a block label for control-flow edits).
    pub target: u32,
    /// The id that now stands for `target` (`None` when it was simply removed or restructured).
    pub replaced_by: Option<u32>,
    pub detail: String,
}

impl EditOp {
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "pass": self.pass,
            "class": self.class.as_str(),
            "target": self.target,
            "replaced_by": self.replaced_by,
            "detail": self.detail,
        })
    }
}

/// Pass options.
#[derive(Clone, Debug)]
pub struct Opts {
    /// `unroll`: maximum trip count that is fully unrolled.
    pub max_unroll: u32,
    /// `select`: maximum number of instructions per arm.
    pub max_select_arm: usize,
    /// Restrict a pass to the given target id (`--only-op`). `dce` ignores it (removing dead
    /// code never changes a value).
    pub only_op: Option<u32>,
    /// Maximum rounds of the `fold,dce,cse,ident` fixed point.
    pub max_rounds: usize,
    /// Skip every edit that would be classed `ulp` (`--exact-only`): `fold` keeps rounding
    /// and transcendental folds, `divconst` keeps non-power-of-two divisors, `powspec` is off.
    pub exact_only: bool,
}

impl Default for Opts {
    fn default() -> Self {
        Opts { max_unroll: 16, max_select_arm: 32, only_op: None, max_rounds: 10, exact_only: false }
    }
}

impl Opts {
    /// True when `id` may be edited under `--only-op`.
    pub fn allows(&self, id: u32) -> bool {
        self.only_op.map_or(true, |o| o == id)
    }
}

pub const CLEANUP: [&str; 4] = ["fold", "dce", "cse", "ident"];
pub const ALL_PASSES: [&str; 8] = ["fold", "dce", "cse", "ident", "unroll", "divconst", "powspec", "select"];

/// Runs one pass by name; returns the number of changes.
pub fn run_pass(name: &str, l: &mut Lifted, ops: &mut Vec<EditOp>, opts: &Opts) -> Result<usize> {
    Ok(match name {
        "fold" => fold::run(l, ops, opts),
        "dce" => dce::run(l, ops, opts),
        "cse" => cse::run(l, ops, opts),
        "ident" => ident::run(l, ops, opts),
        "unroll" => unroll::run(l, ops, opts),
        "divconst" => divconst::run(l, ops, opts),
        "powspec" => powspec::run(l, ops, opts),
        "select" => select::run(l, ops, opts),
        other => bail!("unknown pass {other:?}; passes are {}", ALL_PASSES.join(",")),
    })
}

/// Applies `passes` in order. The cleanup passes among them (`fold,dce,cse,ident`, in their
/// listed order) are repeated to a fixed point (at most `opts.max_rounds` rounds) after every
/// other pass and at the end. Returns the change count per pass name, in first-use order.
pub fn run_pipeline(l: &mut Lifted, passes: &[String], ops: &mut Vec<EditOp>, opts: &Opts) -> Result<Vec<(String, usize)>> {
    for p in passes {
        if !ALL_PASSES.contains(&p.as_str()) {
            bail!("unknown pass {p:?}; passes are {}", ALL_PASSES.join(","));
        }
    }
    let mut counts: Vec<(String, usize)> = Vec::new();
    let bump = |name: &str, n: usize, counts: &mut Vec<(String, usize)>| {
        if let Some(e) = counts.iter_mut().find(|(k, _)| k == name) {
            e.1 += n;
        } else {
            counts.push((name.to_string(), n));
        }
    };
    let cleanup: Vec<String> = {
        let mut v = Vec::new();
        for p in passes {
            if CLEANUP.contains(&p.as_str()) && !v.contains(p) {
                v.push(p.clone());
            }
        }
        v
    };
    let fixed_point = |l: &mut Lifted, ops: &mut Vec<EditOp>, counts: &mut Vec<(String, usize)>| -> Result<()> {
        for _ in 0..opts.max_rounds {
            let mut changed = 0;
            for p in &cleanup {
                let n = run_pass(p, l, ops, opts)?;
                bump(p, n, counts);
                changed += n;
            }
            if changed == 0 {
                break;
            }
        }
        Ok(())
    };
    for p in passes {
        let n = run_pass(p, l, ops, opts)?;
        bump(p, n, &mut counts);
        if !CLEANUP.contains(&p.as_str()) {
            fixed_point(l, ops, &mut counts)?;
        }
    }
    if !cleanup.is_empty() {
        fixed_point(l, ops, &mut counts)?;
    }
    Ok(counts)
}

/// Validates assembled words with the in-process spirv-tools validator.
pub fn validate(words: &[u32]) -> Result<()> {
    use spirv_tools::val::Validator;
    let v = spirv_tools::val::create(None);
    v.validate(words, None).map_err(|e| anyhow!("spirv-val: {e}"))
}

// ---- shared helpers ------------------------------------------------------------------------

/// Allocates a fresh id above the bound and updates the header bound.
pub fn fresh_id(l: &mut Lifted) -> u32 {
    let h = l.module.header.as_mut().expect("module header");
    let id = h.bound;
    h.bound += 1;
    l.bound = h.bound;
    id
}

/// Total number of instructions in function bodies (labels excluded).
pub fn instruction_count(l: &Lifted) -> usize {
    l.module.functions.iter().map(|f| f.blocks.iter().map(|b| b.instructions.len()).sum::<usize>()).sum()
}

pub fn id_op(inst: &dr::Instruction, i: usize) -> Option<u32> {
    inst.operands.get(i).and_then(|o| o.id_ref_any())
}

pub fn lit_op(inst: &dr::Instruction, i: usize) -> Option<u32> {
    match inst.operands.get(i) {
        Some(Operand::LiteralBit32(v)) => Some(*v),
        _ => None,
    }
}

/// The GLSL.std.450 instruction of an `OpExtInst`, if the set is GLSL.std.450.
pub fn glsl_op(l: &Lifted, inst: &dr::Instruction) -> Option<spirv::GLOp> {
    if inst.class.opcode != Op::ExtInst {
        return None;
    }
    let set = id_op(inst, 0)?;
    if l.ext_inst_imports.get(&set).map(|s| s.as_str()) != Some("GLSL.std.450") {
        return None;
    }
    match inst.operands.get(1) {
        Some(Operand::LiteralExtInstInteger(n)) => spirv::GLOp::from_u32(*n),
        _ => None,
    }
}

/// Human-readable spelling of an instruction: `OpFMul %55 %56`, `OpExtInst Exp %47`.
pub fn describe(l: &Lifted, inst: &dr::Instruction) -> String {
    let mut s = String::new();
    if let Some(g) = glsl_op(l, inst) {
        s.push_str(&format!("OpExtInst {g:?}"));
        for o in inst.operands.iter().skip(2) {
            s.push_str(&format!(" {o}"));
        }
    } else {
        s.push_str(&format!("Op{}", inst.class.opname));
        for o in &inst.operands {
            s.push_str(&format!(" {o}"));
        }
    }
    s
}

/// Uses of `id` that are not debug names or decorations.
pub fn real_uses<'a>(l: &'a Lifted, id: u32) -> impl Iterator<Item = &'a Site> + 'a {
    l.uses.get(&id).into_iter().flatten().filter(|s| !matches!(s, Site::DebugName(_) | Site::Annotation(_)))
}

pub fn has_real_uses(l: &Lifted, id: u32) -> bool {
    real_uses(l, id).next().is_some()
}

pub fn inst_at<'a>(l: &'a Lifted, site: Site) -> Option<&'a dr::Instruction> {
    match site {
        Site::Inst(f, b, i) => l.module.functions.get(f)?.blocks.get(b)?.instructions.get(i),
        Site::Global(i) => l.module.types_global_values.get(i),
        _ => None,
    }
}

/// The defining instruction of `id`, wherever it is.
pub fn def_inst<'a>(l: &'a Lifted, id: u32) -> Option<&'a dr::Instruction> {
    inst_at(l, *l.defs.get(&id)?)
}

/// Replaces every id operand `old` by `new` in all function bodies (including phi operands).
pub fn replace_uses(l: &mut Lifted, old: u32, new: u32) {
    for f in &mut l.module.functions {
        for b in &mut f.blocks {
            for inst in &mut b.instructions {
                for o in &mut inst.operands {
                    if o.id_ref_any() == Some(old) {
                        *o = Operand::IdRef(new);
                    }
                }
            }
        }
    }
}

/// Removes the instructions defining the given ids from all function bodies, together with
/// their `OpName`/`OpDecorate` metadata.
pub fn remove_defs(l: &mut Lifted, ids: &HashSet<u32>) {
    if ids.is_empty() {
        return;
    }
    for f in &mut l.module.functions {
        for b in &mut f.blocks {
            b.instructions.retain(|i| !i.result_id.map_or(false, |r| ids.contains(&r)));
        }
    }
    remove_metadata(l, ids);
}

/// Drops `OpName`/`OpMemberName`/`OpDecorate`/`OpMemberDecorate` records targeting the ids.
pub fn remove_metadata(l: &mut Lifted, ids: &HashSet<u32>) {
    let targets = |i: &dr::Instruction| i.operands.first().and_then(|o| o.id_ref_any()).map_or(false, |t| ids.contains(&t));
    l.module.debug_names.retain(|i| !targets(i));
    l.module.annotations.retain(|i| !targets(i));
}

/// Copies every `OpDecorate %from ...` to `%to`.
pub fn copy_decorations(l: &mut Lifted, from: u32, to: u32) {
    let extra: Vec<dr::Instruction> = l
        .module
        .annotations
        .iter()
        .filter(|i| i.class.opcode == Op::Decorate && id_op(i, 0) == Some(from))
        .map(|i| {
            let mut c = i.clone();
            c.operands[0] = Operand::IdRef(to);
            c
        })
        .collect();
    l.module.annotations.extend(extra);
}

/// The storage class of the variable at the root of a pointer (through access chains).
pub fn pointer_root(l: &Lifted, ptr: u32) -> Option<(u32, StorageClass)> {
    let mut cur = ptr;
    for _ in 0..64 {
        let inst = def_inst(l, cur)?;
        match inst.class.opcode {
            Op::Variable => {
                return match inst.operands.first() {
                    Some(Operand::StorageClass(s)) => Some((cur, *s)),
                    _ => None,
                }
            }
            Op::AccessChain | Op::InBoundsAccessChain | Op::PtrAccessChain | Op::CopyObject => {
                cur = id_op(inst, 0)?;
            }
            _ => return None,
        }
    }
    None
}

/// Storage classes whose memory never changes during an invocation.
pub fn is_readonly_storage(s: StorageClass) -> bool {
    matches!(s, StorageClass::Uniform | StorageClass::UniformConstant | StorageClass::PushConstant | StorageClass::Input)
}

/// Result-producing instructions with no side effects: safe to delete when unused.
pub fn is_pure(l: &Lifted, inst: &dr::Instruction) -> bool {
    if inst.result_id.is_none() {
        return false;
    }
    use Op::*;
    match inst.class.opcode {
        Variable => matches!(inst.operands.first(), Some(Operand::StorageClass(StorageClass::Function))),
        ExtInst => glsl_op(l, inst).is_some(),
        Undef | Load | AccessChain | InBoundsAccessChain | PtrAccessChain | ArrayLength | CompositeConstruct
        | CompositeExtract | CompositeInsert | VectorShuffle | VectorExtractDynamic | VectorInsertDynamic | CopyObject
        | CopyLogical | Transpose | SampledImage | Image | ImageSampleImplicitLod | ImageSampleExplicitLod
        | ImageSampleDrefImplicitLod | ImageSampleDrefExplicitLod | ImageSampleProjImplicitLod
        | ImageSampleProjExplicitLod | ImageSampleProjDrefImplicitLod | ImageSampleProjDrefExplicitLod | ImageFetch
        | ImageGather | ImageDrefGather | ImageRead | ImageQuerySize | ImageQuerySizeLod | ImageQueryLevels
        | ImageQueryLod | ImageQueryFormat | ImageQueryOrder | ImageQuerySamples | ConvertFToU | ConvertFToS
        | ConvertSToF | ConvertUToF | UConvert | SConvert | FConvert | QuantizeToF16 | ConvertPtrToU | ConvertUToPtr
        | Bitcast | SNegate | FNegate | IAdd | FAdd | ISub | FSub | IMul | FMul | UDiv | SDiv | FDiv | UMod | SRem
        | SMod | FRem | FMod | VectorTimesScalar | MatrixTimesScalar | VectorTimesMatrix | MatrixTimesVector
        | MatrixTimesMatrix | OuterProduct | Dot | IAddCarry | ISubBorrow | UMulExtended | SMulExtended | Any | All
        | IsNan | IsInf | IsFinite | IsNormal | SignBitSet | LogicalEqual | LogicalNotEqual | LogicalOr | LogicalAnd
        | LogicalNot | Select | IEqual | INotEqual | UGreaterThan | SGreaterThan | UGreaterThanEqual
        | SGreaterThanEqual | ULessThan | SLessThan | ULessThanEqual | SLessThanEqual | FOrdEqual | FUnordEqual
        | FOrdNotEqual | FUnordNotEqual | FOrdLessThan | FUnordLessThan | FOrdGreaterThan | FUnordGreaterThan
        | FOrdLessThanEqual | FUnordLessThanEqual | FOrdGreaterThanEqual | FUnordGreaterThanEqual
        | ShiftRightLogical | ShiftRightArithmetic | ShiftLeftLogical | BitwiseOr | BitwiseXor | BitwiseAnd | Not
        | BitFieldInsert | BitFieldSExtract | BitFieldUExtract | BitReverse | BitCount | DPdx | DPdy | Fwidth
        | DPdxFine | DPdyFine | FwidthFine | DPdxCoarse | DPdyCoarse | FwidthCoarse | Phi | IsHelperInvocationEXT => {
            true
        }
        _ => false,
    }
}

/// Pure instructions whose value depends only on their operands: candidates for `cse`.
/// Loads qualify when the pointer's root variable lives in read-only storage.
pub fn is_cse_candidate(l: &Lifted, inst: &dr::Instruction) -> bool {
    if !is_pure(l, inst) {
        return false;
    }
    use Op::*;
    match inst.class.opcode {
        Variable | Phi | Undef | IsHelperInvocationEXT | DPdx | DPdy | Fwidth | DPdxFine | DPdyFine | FwidthFine
        | DPdxCoarse | DPdyCoarse | FwidthCoarse | SampledImage | Image | ImageSampleImplicitLod
        | ImageSampleExplicitLod | ImageSampleDrefImplicitLod | ImageSampleDrefExplicitLod
        | ImageSampleProjImplicitLod | ImageSampleProjExplicitLod | ImageSampleProjDrefImplicitLod
        | ImageSampleProjDrefExplicitLod | ImageFetch | ImageGather | ImageDrefGather | ImageRead | ImageQuerySize
        | ImageQuerySizeLod | ImageQueryLevels | ImageQueryLod | ImageQueryFormat | ImageQueryOrder
        | ImageQuerySamples | ArrayLength => false,
        Load => {
            // Only a plain load (no memory-access operands) from read-only memory.
            inst.operands.len() == 1
                && id_op(inst, 0).and_then(|p| pointer_root(l, p)).map_or(false, |(_, s)| is_readonly_storage(s))
        }
        _ => true,
    }
}

/// Pure instructions that may execute speculatively (the `select` pass): no image ops,
/// derivatives, interpolation, and loads only through pointers whose every access-chain index
/// is a constant (so no guarded out-of-bounds access is exposed).
pub fn is_speculatable(l: &Lifted, inst: &dr::Instruction) -> bool {
    if !is_pure(l, inst) {
        return false;
    }
    use Op::*;
    match inst.class.opcode {
        Variable | Phi | IsHelperInvocationEXT | DPdx | DPdy | Fwidth | DPdxFine | DPdyFine | FwidthFine
        | DPdxCoarse | DPdyCoarse | FwidthCoarse | SampledImage | Image | ImageSampleImplicitLod
        | ImageSampleExplicitLod | ImageSampleDrefImplicitLod | ImageSampleDrefExplicitLod
        | ImageSampleProjImplicitLod | ImageSampleProjExplicitLod | ImageSampleProjDrefImplicitLod
        | ImageSampleProjDrefExplicitLod | ImageFetch | ImageGather | ImageDrefGather | ImageRead | ImageQuerySize
        | ImageQuerySizeLod | ImageQueryLevels | ImageQueryLod | ImageQueryFormat | ImageQueryOrder
        | ImageQuerySamples | ArrayLength => false,
        ExtInst => !matches!(
            glsl_op(l, inst),
            Some(spirv::GLOp::InterpolateAtCentroid | spirv::GLOp::InterpolateAtSample | spirv::GLOp::InterpolateAtOffset)
        ),
        Load => inst.operands.len() == 1 && id_op(inst, 0).map_or(false, |p| pointer_has_constant_indices(l, p)),
        _ => true,
    }
}

/// True when every access-chain index between `ptr` and its root variable is a constant.
pub fn pointer_has_constant_indices(l: &Lifted, ptr: u32) -> bool {
    let mut cur = ptr;
    for _ in 0..64 {
        let Some(inst) = def_inst(l, cur) else { return false };
        match inst.class.opcode {
            Op::Variable => return true,
            Op::AccessChain | Op::InBoundsAccessChain => {
                if !inst.operands.iter().skip(1).all(|o| o.id_ref_any().map_or(false, |i| l.constants.contains_key(&i))) {
                    return false;
                }
                let Some(b) = id_op(inst, 0) else { return false };
                cur = b;
            }
            _ => return false,
        }
    }
    false
}

/// Element type and count of a scalar/vector type (`(elem, 1)` for a scalar).
pub fn vector_shape(l: &Lifted, ty: u32) -> Option<(u32, u32)> {
    match l.types.get(&ty)? {
        Type::Vector { elem, count } => Some((*elem, *count)),
        Type::Bool | Type::Int { .. } | Type::Float { .. } => Some((ty, 1)),
        _ => None,
    }
}

/// Float width of a scalar or vector float type.
pub fn float_width(l: &Lifted, ty: u32) -> Option<u32> {
    let (elem, _) = vector_shape(l, ty)?;
    match l.types.get(&elem)? {
        Type::Float { width } => Some(*width),
        _ => None,
    }
}

/// Reads the components of a float constant (scalar or vector) as f64 values.
pub fn float_const_components(l: &Lifted, id: u32) -> Option<Vec<f64>> {
    let cv = consts::read(l, id)?;
    let ty = l.constants.get(&id)?.ty;
    let (elem, _) = vector_shape(l, ty)?;
    if !matches!(l.types.get(&elem)?, Type::Float { .. }) {
        return None;
    }
    let comps = match &cv {
        consts::CV::Comp(v) => v.clone(),
        s => vec![s.clone()],
    };
    comps.iter().map(|c| c.as_f64()).collect()
}

/// A map of remapped ids applied to every operand of an instruction.
pub fn remap_operands(inst: &mut dr::Instruction, map: &HashMap<u32, u32>) {
    for o in &mut inst.operands {
        if let Some(id) = o.id_ref_any() {
            if let Some(n) = map.get(&id) {
                *o = Operand::IdRef(*n);
            }
        }
    }
}
