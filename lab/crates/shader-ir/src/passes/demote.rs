//! `demote`: the M3 precision-demotion transform (`lab/CONTRACTS.md`, "M3", `shader-ir demote`).
//!
//! The caller lists f32 float-typed result ids (scalars or vectors) of the input module; which
//! ids are sensible to demote (no address/control sinks, ranges inside f16) is the caller's
//! decision, this pass only checks that the ids exist, are f32-typed and are of a kind it can
//! rewrite. Two forms, both recorded with class `lossy`:
//!
//! * `relaxed`: `OpDecorate %id RelaxedPrecision` for every listed id and nothing else. Mobile
//!   drivers honor it (mediump), desktop drivers usually ignore it.
//! * `f16`: every listed instruction computes in f16. Its result type becomes the f16
//!   counterpart (`f16` / `vecN<f16>`); f32 operands that are not themselves listed get an
//!   `OpFConvert` to f16 inserted right before the instruction (one per (operand, block); the
//!   incoming values of an `OpPhi` convert at the end of the predecessor block, before its
//!   merge/terminator); constant operands become f16 constants (`OpConstant` with the half
//!   bits, round to nearest even); every use of the result by an instruction outside the set
//!   goes through one `OpFConvert` back to f32 placed right after the instruction (after the
//!   phis of the block for a phi). `OpCapability Float16` is added when missing. Listed
//!   instructions keep their result ids; converts, f16 types and constants take fresh ids.
//!
//! Supported kinds: `OpFAdd/FSub/FMul/FDiv/FRem/FMod/FNegate`, `OpDot`, `OpVectorTimesScalar`,
//! `OpCompositeConstruct/Extract/Insert`, `OpVectorShuffle`, `OpVectorExtractDynamic/InsertDynamic`,
//! `OpSelect`, `OpPhi`, `OpCopyObject`, `OpConvertSToF/UToF`, the float `GLSL.std.450`
//! instructions whose float operands and result change type together (`Exp`, `Pow`, `FMix`,
//! `Normalize`, ...), and `OpLoad` of a `Function` variable: listing a load demotes the
//! variable itself (its pointer type becomes `Function f16`, every store to it converts the
//! stored value), which requires every load of that variable to be listed and the variable to
//! be used only by whole-variable loads and stores (no access chains, no call arguments).
//! An `OpFConvert` in the list is skipped (already a conversion).
//!
//! Rejected with a message naming the id and the reason: comparisons (`OpFOrd*`/`OpFUnord*`),
//! image ops, derivatives, matrix ops, loads of `Uniform`/`Input`/`PushConstant`/`UniformConstant`/
//! `Output`/`Private` variables and loads through access chains, function parameters and call
//! results, mixed-signature `GLSL.std.450` ops (`Ldexp`, `Frexp`, `Modf`, packing, matrix
//! functions), operands of matrix/struct/array type and specialization-constant/undef operands.
//! [`prune`] applies the same rules to a candidate list and returns the survivors (the corpus
//! test uses it), so a caller can demote "everything demotable" in one call.
//!
//! `--group-converts` ([`group_converts`]) then removes `f16 -> f32 -> f16` pairs (an
//! `OpFConvert` of an `OpFConvert` back to the original type when the intermediate has no other
//! use; class `exact`, the wider type represents every narrower value). A single `demote` call
//! never produces such pairs (an operand that is itself listed is used directly), they arise
//! when demotion is applied incrementally to an already demoted module.

use super::consts::{self, CV};
use super::{def_inst, describe, float_width, fresh_id, glsl_op, id_op, inst_at, pointer_root, real_uses, remove_defs, replace_uses, vector_shape, Class, EditOp};
use crate::lift::{ConstKind, Lifted, Site, Type};
use anyhow::{anyhow, bail, Result};
use rspirv::dr::{Instruction, Operand};
use spirv::{Capability, Decoration, GLOp, Op, StorageClass};
use std::collections::{HashMap, HashSet};

/// `--mode relaxed | f16`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DemoteMode {
    Relaxed,
    F16,
}

impl std::str::FromStr for DemoteMode {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        Ok(match s {
            "relaxed" => DemoteMode::Relaxed,
            "f16" => DemoteMode::F16,
            other => bail!("unknown --mode {other:?}; want relaxed or f16"),
        })
    }
}

/// What one `demote` call did.
#[derive(Clone, Debug, Default)]
pub struct Report {
    /// Listed ids that were demoted (relaxed: decorated).
    pub demoted: usize,
    /// `OpFConvert` f32 -> f16 inserted (operands entering the demoted set).
    pub converts_in: usize,
    /// `OpFConvert` f16 -> f32 inserted (results leaving the demoted set).
    pub converts_out: usize,
    /// Conversion pairs removed by `--group-converts`.
    pub converts_removed: usize,
    /// Listed ids that needed no edit (already an `OpFConvert`, already decorated).
    pub skipped: Vec<(u32, String)>,
}

/// Demotes `sites` in place; records one [`EditOp`] per demoted instruction (and one per
/// removed conversion pair). Errors list every offending id.
pub fn run(l: &mut Lifted, sites: &[u32], mode: DemoteMode, group: bool, ops: &mut Vec<EditOp>) -> Result<Report> {
    check_sites(l, sites)?;
    match mode {
        DemoteMode::Relaxed => run_relaxed(l, sites, ops),
        DemoteMode::F16 => {
            let mut r = run_f16(l, sites, ops)?;
            if group {
                r.converts_removed = group_converts(l, ops);
            }
            Ok(r)
        }
    }
}

/// True for an f32 scalar or f32 vector type.
pub fn is_f32_value(l: &Lifted, ty: u32) -> bool {
    float_width(l, ty) == Some(32)
}

fn is_comparison(op: Op) -> bool {
    use Op::*;
    matches!(
        op,
        FOrdEqual | FUnordEqual | FOrdNotEqual | FUnordNotEqual | FOrdLessThan | FUnordLessThan | FOrdGreaterThan
            | FUnordGreaterThan | FOrdLessThanEqual | FUnordLessThanEqual | FOrdGreaterThanEqual | FUnordGreaterThanEqual
    )
}

fn type_reason(l: &Lifted, id: u32) -> Option<String> {
    if let Some(inst) = def_inst(l, id) {
        if is_comparison(inst.class.opcode) {
            return Some(format!("is Op{}: comparisons feed control flow and keep their f32 operands (bool result)", inst.class.opname));
        }
    }
    match l.result_types.get(&id) {
        None => Some(if l.defs.contains_key(&id) {
            let what = def_inst(l, id).map(|i| format!("Op{}", i.class.opname)).unwrap_or_else(|| "a label or parameter".into());
            format!("has no value type ({what})")
        } else {
            format!("does not exist (bound {})", l.bound)
        }),
        Some(&ty) if !is_f32_value(l, ty) => Some(format!("is not f32-typed ({})", l.type_name(ty))),
        _ => None,
    }
}

/// Every id must exist and be an f32 scalar/vector result; the error lists all offenders.
pub fn check_sites(l: &Lifted, sites: &[u32]) -> Result<()> {
    let bad: Vec<String> = sites.iter().filter_map(|&id| type_reason(l, id).map(|r| format!("%{id} {r}"))).collect();
    if !bad.is_empty() {
        bail!("--sites: {}", bad.join("; "));
    }
    Ok(())
}

// ---- relaxed ------------------------------------------------------------------------------

fn run_relaxed(l: &mut Lifted, sites: &[u32], ops: &mut Vec<EditOp>) -> Result<Report> {
    let mut report = Report::default();
    let mut seen = HashSet::new();
    for &id in sites {
        if !seen.insert(id) {
            continue;
        }
        if l.has_decoration(id, Decoration::RelaxedPrecision) {
            report.skipped.push((id, "already RelaxedPrecision".into()));
            continue;
        }
        let what = def_inst(l, id).map(|i| describe(l, i)).unwrap_or_default();
        l.module.annotations.push(Instruction::new(Op::Decorate, None, None, vec![Operand::IdRef(id), Operand::Decoration(Decoration::RelaxedPrecision)]));
        ops.push(EditOp { pass: "demote_relaxed", class: Class::Lossy, target: id, replaced_by: Some(id), detail: format!("OpDecorate %{id} RelaxedPrecision ({what})") });
        report.demoted += 1;
    }
    l.reanalyze()?;
    Ok(report)
}

// ---- f16: kind checks ---------------------------------------------------------------------

const GLSL_F16_OK: &[GLOp] = &[
    GLOp::Round, GLOp::RoundEven, GLOp::Trunc, GLOp::FAbs, GLOp::FSign, GLOp::Floor, GLOp::Ceil, GLOp::Fract,
    GLOp::Radians, GLOp::Degrees, GLOp::Sin, GLOp::Cos, GLOp::Tan, GLOp::Asin, GLOp::Acos, GLOp::Atan, GLOp::Sinh,
    GLOp::Cosh, GLOp::Tanh, GLOp::Asinh, GLOp::Acosh, GLOp::Atanh, GLOp::Atan2, GLOp::Pow, GLOp::Exp, GLOp::Log,
    GLOp::Exp2, GLOp::Log2, GLOp::Sqrt, GLOp::InverseSqrt, GLOp::FMin, GLOp::FMax, GLOp::FClamp, GLOp::FMix,
    GLOp::Step, GLOp::SmoothStep, GLOp::Fma, GLOp::Length, GLOp::Distance, GLOp::Cross, GLOp::Normalize,
    GLOp::FaceForward, GLOp::Reflect, GLOp::Refract, GLOp::NMin, GLOp::NMax, GLOp::NClamp,
];

/// True when a listed id is an `OpFConvert`: skipped, not an error.
pub fn is_skip(l: &Lifted, id: u32) -> bool {
    def_inst(l, id).map_or(false, |i| i.class.opcode == Op::FConvert)
}

fn var_name(l: &Lifted, var: u32) -> String {
    match l.name(var) {
        Some(n) => format!("%{var} (\"{n}\")"),
        None => format!("%{var}"),
    }
}

/// Why `id` (an existing f32-typed result) cannot be demoted to f16 given the set of listed
/// ids, or `None` when it can.
pub fn reject_reason(l: &Lifted, id: u32, set: &HashSet<u32>) -> Option<String> {
    let site = *l.defs.get(&id)?;
    if matches!(site, Site::Parameter(..)) {
        return Some("OpFunctionParameter: function signatures keep their f32 types".into());
    }
    let inst = inst_at(l, site)?;
    use Op::*;
    let op = inst.class.opcode;
    let name = format!("Op{}", inst.class.opname);
    if is_comparison(op) {
        return Some(format!("{name}: comparisons feed control flow and keep their f32 operands"));
    }
    match op {
        ImageSampleImplicitLod | ImageSampleExplicitLod | ImageSampleDrefImplicitLod | ImageSampleDrefExplicitLod
        | ImageSampleProjImplicitLod | ImageSampleProjExplicitLod | ImageSampleProjDrefImplicitLod
        | ImageSampleProjDrefExplicitLod | ImageFetch | ImageGather | ImageDrefGather | ImageRead | ImageQueryLod
        | ImageQuerySize | ImageQuerySizeLod | SampledImage | Image => {
            return Some(format!("{name}: image ops keep their f32 result (interface type)"));
        }
        DPdx | DPdy | Fwidth | DPdxFine | DPdyFine | FwidthFine | DPdxCoarse | DPdyCoarse | FwidthCoarse => {
            return Some(format!("{name}: derivatives are not demoted"));
        }
        MatrixTimesScalar | VectorTimesMatrix | MatrixTimesVector | MatrixTimesMatrix | OuterProduct | Transpose => {
            return Some(format!("{name}: matrix ops are not demoted"));
        }
        FunctionCall => return Some(format!("{name}: call results keep the callee's f32 signature")),
        FConvert => return Some(format!("{name}: already a conversion")),
        Load => return load_reason(l, inst, set),
        ExtInst => match glsl_op(l, inst) {
            Some(g) if GLSL_F16_OK.contains(&g) => {}
            Some(g) => return Some(format!("GLSL.std.450 {g:?}: mixed-signature or non-float instruction is not demoted")),
            None => return Some("OpExtInst of a set other than GLSL.std.450".into()),
        },
        FAdd | FSub | FMul | FDiv | FRem | FMod | FNegate | Dot | VectorTimesScalar | CompositeConstruct | CompositeExtract
        | CompositeInsert | VectorShuffle | VectorExtractDynamic | VectorInsertDynamic | Select | Phi | CopyObject
        | ConvertSToF | ConvertUToF => {}
        _ => return Some(format!("{name}: not a demotable instruction kind")),
    }
    operand_reason(l, inst, set)
}

/// Operands must be f32 values (listed or convertible, constants readable), f16 values, or
/// non-float scalars/vectors (conditions, indices) that stay as they are.
fn operand_reason(l: &Lifted, inst: &Instruction, set: &HashSet<u32>) -> Option<String> {
    for o in &inst.operands {
        let Some(x) = o.id_ref_any() else { continue };
        if set.contains(&x) {
            continue;
        }
        let Some(&t) = l.result_types.get(&x) else { continue };
        if is_f32_value(l, t) {
            if l.constants.contains_key(&x) && consts::read(l, x).is_none() {
                return Some(format!("operand %{x} is a specialization constant or undef"));
            }
            continue;
        }
        match vector_shape(l, t).and_then(|(e, _)| l.types.get(&e)) {
            Some(Type::Bool) | Some(Type::Int { .. }) | Some(Type::Float { width: 16 }) => {}
            _ => return Some(format!("operand %{x} has type {}: only f32/f16/bool/int scalars and vectors are demotable", l.type_name(t))),
        }
    }
    None
}

fn load_reason(l: &Lifted, inst: &Instruction, set: &HashSet<u32>) -> Option<String> {
    let ptr = id_op(inst, 0)?;
    let Some((var, storage)) = pointer_root(l, ptr) else {
        return Some(if matches!(l.defs.get(&ptr), Some(Site::Parameter(..))) {
            format!("OpLoad through the pointer parameter %{ptr}: function signatures keep their f32 types")
        } else {
            format!("OpLoad through %{ptr}, which is not rooted in a variable")
        });
    };
    if storage != StorageClass::Function {
        return Some(format!("OpLoad of a {storage:?} variable {}: interface types must not change", var_name(l, var)));
    }
    if ptr != var {
        return Some(format!("OpLoad through an access chain into {}: only whole-variable loads are demotable", var_name(l, var)));
    }
    let mut unlisted = Vec::new();
    for s in real_uses(l, var) {
        let Some(u) = inst_at(l, *s) else { continue };
        match u.class.opcode {
            Op::Load => {
                if let Some(r) = u.result_id {
                    if !set.contains(&r) {
                        unlisted.push(format!("%{r}"));
                    }
                }
            }
            Op::Store if id_op(u, 0) == Some(var) && u.operands.len() == 2 => {}
            _ => {
                return Some(format!(
                    "variable {} is also used by Op{}: only whole-variable loads and stores are demotable",
                    var_name(l, var),
                    u.class.opname
                ))
            }
        }
    }
    if !unlisted.is_empty() {
        return Some(format!("variable {}: loads {} are not listed; list every load of a variable or none", var_name(l, var), unlisted.join(", ")));
    }
    if let Some(init) = def_inst(l, var).and_then(|v| id_op(v, 1)) {
        if consts::read(l, init).is_none() {
            return Some(format!("variable {} has a non-constant initializer %{init}", var_name(l, var)));
        }
    }
    None
}

/// Drops from `sites` every id the f16 form would reject (or skip), to a fixed point (dropping
/// one load of a variable unlists the variable, which drops its other loads). Returns the
/// survivors in the given order and the dropped ids with their reasons.
pub fn prune(l: &Lifted, sites: &[u32]) -> (Vec<u32>, Vec<(u32, String)>) {
    let mut keep: Vec<u32> = Vec::new();
    let mut rejected = Vec::new();
    let mut seen = HashSet::new();
    for &id in sites {
        if !seen.insert(id) {
            continue;
        }
        match type_reason(l, id) {
            Some(r) => rejected.push((id, r)),
            None => keep.push(id),
        }
    }
    loop {
        let set: HashSet<u32> = keep.iter().copied().collect();
        let mut removed = false;
        keep.retain(|&id| match reject_reason(l, id, &set) {
            Some(r) => {
                rejected.push((id, r));
                removed = true;
                false
            }
            None => true,
        });
        if !removed {
            break;
        }
    }
    (keep, rejected)
}

// ---- f16: types and constants -------------------------------------------------------------

/// f16 types and constants of the module (existing ones found, new ones appended on demand).
struct F16 {
    scalar: Option<u32>,
    /// component count -> vector type id (1 -> scalar).
    vectors: HashMap<u32, u32>,
    /// pointee type -> `OpTypePointer Function` id.
    pointers: HashMap<u32, u32>,
    /// (type, half bits) -> scalar constant id.
    scalars: HashMap<u16, u32>,
    /// (vector type, component ids) -> composite id.
    composites: HashMap<(u32, Vec<u32>), u32>,
}

impl F16 {
    fn scan(l: &Lifted) -> Self {
        let mut t = F16 { scalar: None, vectors: HashMap::new(), pointers: HashMap::new(), scalars: HashMap::new(), composites: HashMap::new() };
        // First (lowest id) f16 scalar type wins; rspirv keeps declaration order in the
        // module, so pick the earliest declared one.
        for inst in &l.module.types_global_values {
            if inst.class.opcode == Op::TypeFloat && inst.operands.first() == Some(&Operand::LiteralBit32(16)) {
                t.scalar = inst.result_id;
                break;
            }
        }
        let Some(s) = t.scalar else { return t };
        t.vectors.insert(1, s);
        for inst in &l.module.types_global_values {
            let Some(id) = inst.result_id else { continue };
            match inst.class.opcode {
                Op::TypeVector => {
                    if let (Some(e), Some(Operand::LiteralBit32(n))) = (id_op(inst, 0), inst.operands.get(1)) {
                        if e == s && !t.vectors.contains_key(n) {
                            t.vectors.insert(*n, id);
                        }
                    }
                }
                Op::TypePointer => {
                    if let (Some(Operand::StorageClass(StorageClass::Function)), Some(p)) = (inst.operands.first(), id_op(inst, 1)) {
                        t.pointers.entry(p).or_insert(id);
                    }
                }
                _ => {}
            }
        }
        for (id, c) in &l.constants {
            if c.spec {
                continue;
            }
            match &c.kind {
                ConstKind::Bits32(b) if c.ty == s => {
                    t.scalars.entry(*b as u16).or_insert(*id);
                }
                ConstKind::Composite(ids) if t.vectors.values().any(|v| *v == c.ty) => {
                    t.composites.entry((c.ty, ids.clone())).or_insert(*id);
                }
                _ => {}
            }
        }
        t
    }

    fn scalar(&mut self, l: &mut Lifted) -> u32 {
        if let Some(s) = self.scalar {
            return s;
        }
        let id = fresh_id(l);
        l.module.types_global_values.push(Instruction::new(Op::TypeFloat, None, Some(id), vec![Operand::LiteralBit32(16)]));
        self.scalar = Some(id);
        self.vectors.insert(1, id);
        id
    }

    /// The f16 counterpart of an f32 scalar/vector type.
    fn of(&mut self, l: &mut Lifted, f32_ty: u32) -> u32 {
        let n = vector_shape(l, f32_ty).map(|(_, n)| n).unwrap_or(1);
        let s = self.scalar(l);
        if let Some(v) = self.vectors.get(&n) {
            return *v;
        }
        let id = fresh_id(l);
        l.module.types_global_values.push(Instruction::new(Op::TypeVector, None, Some(id), vec![Operand::IdRef(s), Operand::LiteralBit32(n)]));
        self.vectors.insert(n, id);
        id
    }

    fn pointer(&mut self, l: &mut Lifted, pointee: u32) -> u32 {
        if let Some(p) = self.pointers.get(&pointee) {
            return *p;
        }
        let id = fresh_id(l);
        l.module.types_global_values.push(Instruction::new(Op::TypePointer, None, Some(id), vec![Operand::StorageClass(StorageClass::Function), Operand::IdRef(pointee)]));
        self.pointers.insert(pointee, id);
        id
    }

    fn scalar_const(&mut self, l: &mut Lifted, x: f32) -> u32 {
        let bits = half::f16::from_f32(x).to_bits();
        if let Some(c) = self.scalars.get(&bits) {
            return *c;
        }
        let s = self.scalar(l);
        let id = fresh_id(l);
        l.module.types_global_values.push(Instruction::new(Op::Constant, Some(s), Some(id), vec![Operand::LiteralBit32(bits as u32)]));
        self.scalars.insert(bits, id);
        id
    }

    /// The f16 constant with the value of the f32 constant `c` (RNE per component).
    fn constant(&mut self, l: &mut Lifted, c: u32) -> Result<u32> {
        let cv = consts::read(l, c).ok_or_else(|| anyhow!("%{c} is not a readable constant"))?;
        let ty = l.constants.get(&c).map(|k| k.ty).ok_or_else(|| anyhow!("%{c} is not a constant"))?;
        match cv {
            CV::F32(x) => Ok(self.scalar_const(l, x)),
            CV::Comp(items) => {
                let mut ids = Vec::with_capacity(items.len());
                for it in &items {
                    match it {
                        CV::F32(x) => ids.push(self.scalar_const(l, *x)),
                        _ => bail!("%{c}: not an f32 vector constant"),
                    }
                }
                let vty = self.of(l, ty);
                if let Some(id) = self.composites.get(&(vty, ids.clone())) {
                    return Ok(*id);
                }
                let id = fresh_id(l);
                l.module.types_global_values.push(Instruction::new(Op::ConstantComposite, Some(vty), Some(id), ids.iter().map(|i| Operand::IdRef(*i)).collect()));
                self.composites.insert((vty, ids), id);
                Ok(id)
            }
            _ => bail!("%{c}: not an f32 constant"),
        }
    }
}

fn fconvert(ty: u32, id: u32, from: u32) -> Instruction {
    Instruction::new(Op::FConvert, Some(ty), Some(id), vec![Operand::IdRef(from)])
}

// ---- f16: the rewrite ----------------------------------------------------------------------

fn run_f16(l: &mut Lifted, sites: &[u32], ops: &mut Vec<EditOp>) -> Result<Report> {
    let mut report = Report::default();
    let mut order: Vec<u32> = Vec::new();
    let mut set: HashSet<u32> = HashSet::new();
    for &id in sites {
        if set.insert(id) {
            order.push(id);
        }
    }
    for &id in &order {
        if is_skip(l, id) {
            set.remove(&id);
            report.skipped.push((id, "already an OpFConvert".into()));
        }
    }
    let mut errors = Vec::new();
    for &id in &order {
        if !set.contains(&id) {
            continue;
        }
        if let Some(r) = reject_reason(l, id, &set) {
            errors.push(format!("%{id}: {r}"));
        }
    }
    if !errors.is_empty() {
        bail!("demote f16: {} site(s) rejected:\n  {}", errors.len(), errors.join("\n  "));
    }
    if set.is_empty() {
        return Ok(report);
    }

    // Variables demoted through their loads.
    let mut vars: HashSet<u32> = HashSet::new();
    for &id in &set {
        if let Some(inst) = def_inst(l, id) {
            if inst.class.opcode == Op::Load {
                if let Some(p) = id_op(inst, 0) {
                    vars.insert(p);
                }
            }
        }
    }
    let is_demoted_inst = |inst: &Instruction| -> bool {
        match inst.class.opcode {
            Op::Store => id_op(inst, 0).map_or(false, |p| vars.contains(&p)),
            Op::Variable => inst.result_id.map_or(false, |r| vars.contains(&r)),
            _ => inst.result_id.map_or(false, |r| set.contains(&r)),
        }
    };

    // Results that leave the set need one convert back to f32.
    let mut back: HashMap<u32, u32> = HashMap::new();
    let mut needs_back: Vec<u32> = Vec::new();
    for &id in &order {
        if !set.contains(&id) {
            continue;
        }
        let leaves = real_uses(l, id).any(|s| inst_at(l, *s).map_or(true, |u| !is_demoted_inst(u)));
        if leaves {
            needs_back.push(id);
        }
    }
    for id in needs_back {
        let b = fresh_id(l);
        back.insert(id, b);
    }

    // Phi incoming values from outside the set convert at the end of the predecessor block.
    // (value, predecessor label) -> convert id; per (function, block): the converts to emit.
    let mut phi_needs: Vec<(u32, u32)> = Vec::new();
    let mut phi_extra: HashMap<u32, usize> = HashMap::new();
    for f in &l.module.functions {
        for b in &f.blocks {
            for inst in &b.instructions {
                if inst.class.opcode != Op::Phi || !inst.result_id.map_or(false, |r| set.contains(&r)) {
                    continue;
                }
                for pair in inst.operands.chunks(2) {
                    let (Some(x), Some(label)) = (pair[0].id_ref_any(), pair.get(1).and_then(|o| o.id_ref_any())) else { continue };
                    if set.contains(&x) || l.constants.contains_key(&x) || !l.result_types.get(&x).map_or(false, |&t| is_f32_value(l, t)) {
                        continue;
                    }
                    if !phi_needs.contains(&(x, label)) {
                        phi_needs.push((x, label));
                        *phi_extra.entry(inst.result_id.unwrap()).or_default() += 1;
                    }
                }
            }
        }
    }
    let mut phi_conv: HashMap<(u32, u32), u32> = HashMap::new();
    let mut phi_end: HashMap<(usize, usize), Vec<(u32, u32)>> = HashMap::new();
    for (x, label) in phi_needs {
        let id = fresh_id(l);
        phi_conv.insert((x, label), id);
        let (fi, bi) = *l.label_index.get(&label).ok_or_else(|| anyhow!("phi predecessor %{label} is not a label"))?;
        phi_end.entry((fi, bi)).or_default().push((x, id));
    }

    let mut f16 = F16::scan(l);
    let mut alias: Vec<(u32, u32)> = Vec::new();
    let nf = l.module.functions.len();
    for fi in 0..nf {
        let nb = l.module.functions[fi].blocks.len();
        for bi in 0..nb {
            let old = std::mem::take(&mut l.module.functions[fi].blocks[bi].instructions);
            let mut out: Vec<Instruction> = Vec::with_capacity(old.len() + 8);
            let mut in_map: HashMap<u32, u32> = HashMap::new();
            let mut phi_backs: Vec<Instruction> = Vec::new();
            for inst in old {
                let op = inst.class.opcode;
                if !matches!(op, Op::Phi | Op::Line | Op::NoLine) && !phi_backs.is_empty() {
                    out.append(&mut phi_backs);
                }
                if !is_demoted_inst(&inst) {
                    let mut ni = inst;
                    for o in &mut ni.operands {
                        if let Some(b) = o.id_ref_any().and_then(|x| back.get(&x)) {
                            *o = Operand::IdRef(*b);
                        }
                    }
                    out.push(ni);
                    continue;
                }
                let desc = describe(l, &inst);
                let rid = inst.result_id;
                let rty = inst.result_type;
                let store_var = if op == Op::Store { id_op(&inst, 0) } else { None };
                let mut ni = inst;
                let mut nconv = 0usize;
                match op {
                    Op::Variable => {
                        let pointee = match l.types.get(&rty.unwrap_or(0)) {
                            Some(Type::Pointer { pointee, .. }) => *pointee,
                            _ => bail!("variable %{} has no pointer type", rid.unwrap_or(0)),
                        };
                        let p16 = f16.of(l, pointee);
                        ni.result_type = Some(f16.pointer(l, p16));
                        if let Some(init) = id_op(&ni, 1) {
                            ni.operands[1] = Operand::IdRef(f16.constant(l, init)?);
                        }
                    }
                    Op::Phi => {
                        let n = ni.operands.len();
                        for i in (0..n).step_by(2) {
                            let (Some(x), Some(label)) = (ni.operands[i].id_ref_any(), ni.operands.get(i + 1).and_then(|o| o.id_ref_any())) else { continue };
                            if set.contains(&x) || !l.result_types.get(&x).map_or(false, |&t| is_f32_value(l, t)) {
                                continue;
                            }
                            let c = if l.constants.contains_key(&x) { f16.constant(l, x)? } else { phi_conv[&(x, label)] };
                            ni.operands[i] = Operand::IdRef(c);
                        }
                        nconv += phi_extra.get(&rid.unwrap_or(0)).copied().unwrap_or(0);
                    }
                    _ => {
                        for i in 0..ni.operands.len() {
                            let Some(x) = ni.operands[i].id_ref_any() else { continue };
                            if set.contains(&x) {
                                continue;
                            }
                            let Some(&t) = l.result_types.get(&x) else { continue };
                            if !is_f32_value(l, t) {
                                continue;
                            }
                            let c = if l.constants.contains_key(&x) {
                                f16.constant(l, x)?
                            } else if let Some(c) = in_map.get(&x) {
                                *c
                            } else {
                                let t16 = f16.of(l, t);
                                let c = fresh_id(l);
                                out.push(fconvert(t16, c, x));
                                in_map.insert(x, c);
                                report.converts_in += 1;
                                nconv += 1;
                                c
                            };
                            ni.operands[i] = Operand::IdRef(c);
                        }
                    }
                }
                if op != Op::Variable {
                    if let Some(t) = rty {
                        ni.result_type = Some(f16.of(l, t));
                    }
                }
                out.push(ni);
                if let Some(r) = rid {
                    if let Some(&b) = back.get(&r) {
                        let conv = fconvert(rty.unwrap(), b, r);
                        if op == Op::Phi {
                            phi_backs.push(conv);
                        } else {
                            out.push(conv);
                        }
                        report.converts_out += 1;
                        nconv += 1;
                    }
                }
                let (target, detail) = match op {
                    Op::Variable => {
                        let v = rid.unwrap();
                        (v, format!("OpVariable {} Function f32->f16 (demoted through its loads)", var_name(l, v)))
                    }
                    Op::Store => {
                        let v = store_var.unwrap();
                        (v, format!("{desc} to {} f32->f16 (+{nconv} converts)", var_name(l, v)))
                    }
                    _ => {
                        report.demoted += 1;
                        (rid.unwrap(), format!("{desc} f32->f16 (+{nconv} converts)"))
                    }
                };
                ops.push(EditOp { pass: "demote_f16", class: Class::Lossy, target, replaced_by: Some(target), detail });
            }
            if !phi_backs.is_empty() {
                out.append(&mut phi_backs);
            }
            if let Some(list) = phi_end.get(&(fi, bi)) {
                let mut ins = Vec::new();
                for &(x, newid) in list {
                    if let Some(&c) = in_map.get(&x) {
                        alias.push((newid, c));
                    } else {
                        let t = l.result_types[&x];
                        let t16 = f16.of(l, t);
                        ins.push(fconvert(t16, newid, x));
                        report.converts_in += 1;
                    }
                }
                let mut idx = out.len().saturating_sub(1);
                if idx > 0 && matches!(out[idx - 1].class.opcode, Op::SelectionMerge | Op::LoopMerge) {
                    idx -= 1;
                }
                out.splice(idx..idx, ins);
            }
            l.module.functions[fi].blocks[bi].instructions = out;
        }
    }
    for (from, to) in alias {
        replace_uses(l, from, to);
    }
    let has_cap = l.module.capabilities.iter().any(|c| c.operands.first() == Some(&Operand::Capability(Capability::Float16)));
    if !has_cap {
        l.module.capabilities.push(Instruction::new(Op::Capability, None, None, vec![Operand::Capability(Capability::Float16)]));
    }
    l.reanalyze()?;
    Ok(report)
}

// ---- group converts ------------------------------------------------------------------------

/// Removes `OpFConvert` pairs that widen and narrow back to the original type (`f16 -> f32 ->
/// f16`) when the intermediate has no other use; iterated to a fixed point. Returns the number
/// of pairs removed (one `exact` [`EditOp`] each).
pub fn group_converts(l: &mut Lifted, ops: &mut Vec<EditOp>) -> usize {
    let mut total = 0;
    loop {
        let mut pairs: Vec<(u32, u32, u32)> = Vec::new();
        let mut taken: HashSet<u32> = HashSet::new();
        for f in &l.module.functions {
            for b in &f.blocks {
                for inst in &b.instructions {
                    if inst.class.opcode != Op::FConvert {
                        continue;
                    }
                    let (Some(bid), Some(bty), Some(a)) = (inst.result_id, inst.result_type, id_op(inst, 0)) else { continue };
                    let Some(ai) = def_inst(l, a) else { continue };
                    if ai.class.opcode != Op::FConvert {
                        continue;
                    }
                    let (Some(aty), Some(x)) = (ai.result_type, id_op(ai, 0)) else { continue };
                    if l.result_types.get(&x) != Some(&bty) || float_width(l, aty) < float_width(l, bty) {
                        continue;
                    }
                    if real_uses(l, a).count() != 1 || !taken.insert(a) || taken.contains(&bid) {
                        continue;
                    }
                    taken.insert(bid);
                    pairs.push((bid, a, x));
                }
            }
        }
        if pairs.is_empty() {
            break;
        }
        for &(b, a, x) in &pairs {
            let detail = format!(
                "OpFConvert %{a} ({} -> {}) and OpFConvert %{b} (-> {}) of %{x} removed: pair widens and narrows back",
                l.type_name(l.result_types[&x]),
                l.type_name(l.result_types[&a]),
                l.type_name(l.result_types[&b])
            );
            replace_uses(l, b, x);
            ops.push(EditOp { pass: "group_converts", class: Class::Exact, target: b, replaced_by: Some(x), detail });
        }
        let ids: HashSet<u32> = pairs.iter().flat_map(|&(b, a, _)| [b, a]).collect();
        remove_defs(l, &ids);
        total += pairs.len();
        l.reanalyze().expect("reanalyze after group_converts");
    }
    total
}
