//! Constant values for the rewrite passes: reading `OpConstant*`, interning new constants
//! (deduplicated against the module's existing ones) and evaluating instructions whose operands
//! are all constants.
//!
//! Float folds are computed in the operand's precision: f32 ops with Rust `f32` arithmetic and
//! functions, f64 ops in `f64`. An f32 fold is classed `exact` only when the f32 result equals
//! the same operation evaluated in f64 (no rounding happened), so that the lab's f64 reference
//! interpreter is unchanged by it; a fold that did round is `ulp` (it is still bit-identical on
//! IEEE f32 hardware for +,-,*,/, but not against the f64 reference). Transcendentals (`exp`,
//! `pow`, `sin`, ...) folded on the CPU may differ from the GPU's implementation by ULPs and are
//! always `ulp`.

use super::{glsl_op, Class};
use crate::lift::{ConstKind, Constant, Lifted, Type};
use rspirv::dr::{self, Operand};
use spirv::{GLOp, Op};

/// A constant value.
#[derive(Clone, Debug, PartialEq)]
pub enum CV {
    Bool(bool),
    /// 32-bit integer bits (signedness comes from the type).
    I(u32),
    F32(f32),
    F64(f64),
    Comp(Vec<CV>),
}

impl CV {
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            CV::F32(x) => Some(*x as f64),
            CV::F64(x) => Some(*x),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            CV::Bool(b) => Some(*b),
            _ => None,
        }
    }
    pub fn as_i(&self) -> Option<u32> {
        match self {
            CV::I(x) => Some(*x),
            _ => None,
        }
    }
    pub fn comps(&self) -> Vec<CV> {
        match self {
            CV::Comp(v) => v.clone(),
            s => vec![s.clone()],
        }
    }
    /// Bitwise equality (distinguishes -0.0 from 0.0, treats equal NaN bit patterns as equal).
    pub fn bits_eq(&self, o: &CV) -> bool {
        match (self, o) {
            (CV::Bool(a), CV::Bool(b)) => a == b,
            (CV::I(a), CV::I(b)) => a == b,
            (CV::F32(a), CV::F32(b)) => a.to_bits() == b.to_bits(),
            (CV::F64(a), CV::F64(b)) => a.to_bits() == b.to_bits(),
            (CV::Comp(a), CV::Comp(b)) => a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.bits_eq(y)),
            _ => false,
        }
    }
    /// True when every scalar component equals `v` (a splat or scalar).
    pub fn is_splat_f(&self, v: f64) -> bool {
        let c = self.comps();
        !c.is_empty() && c.iter().all(|x| x.as_f64() == Some(v))
    }
    pub fn is_splat_i(&self, v: u32) -> bool {
        let c = self.comps();
        !c.is_empty() && c.iter().all(|x| x.as_i() == Some(v))
    }
}

/// Human-readable value: `0.5`, `(1, 2, 3)`, `true`.
pub fn fmt_cv(v: &CV) -> String {
    match v {
        CV::Bool(b) => b.to_string(),
        CV::I(x) => {
            if (*x as i32) < 0 {
                format!("{}", *x as i32)
            } else {
                x.to_string()
            }
        }
        CV::F32(x) => fmt_f(*x as f64),
        CV::F64(x) => fmt_f(*x),
        CV::Comp(v) => format!("({})", v.iter().map(fmt_cv).collect::<Vec<_>>().join(", ")),
    }
}

fn fmt_f(x: f64) -> String {
    if x.is_finite() && x == x.trunc() && x.abs() < 1e15 {
        format!("{x:.1}")
    } else {
        format!("{x}")
    }
}

/// Zero of a type.
pub fn zero(l: &Lifted, ty: u32) -> Option<CV> {
    Some(match l.types.get(&ty)? {
        Type::Bool => CV::Bool(false),
        Type::Int { width: 32, .. } => CV::I(0),
        Type::Float { width: 32 } => CV::F32(0.0),
        Type::Float { width: 64 } => CV::F64(0.0),
        Type::Vector { elem, count } => CV::Comp(vec![zero(l, *elem)?; *count as usize]),
        Type::Matrix { column, columns } => CV::Comp(vec![zero(l, *column)?; *columns as usize]),
        Type::Array { elem, len } => CV::Comp(vec![zero(l, *elem)?; l.array_len(*len).ok()? as usize]),
        Type::Struct { members } => CV::Comp(members.iter().map(|m| zero(l, *m)).collect::<Option<_>>()?),
        _ => return None,
    })
}

/// Reads a non-specialization constant. `OpConstantNull` reads as zero; `OpUndef` does not
/// read (folding through undef is never attempted).
pub fn read(l: &Lifted, id: u32) -> Option<CV> {
    let c: &Constant = l.constants.get(&id)?;
    if c.spec {
        return None;
    }
    Some(match &c.kind {
        ConstKind::Bool(b) => CV::Bool(*b),
        ConstKind::Bits32(bits) => match l.types.get(&c.ty)? {
            Type::Int { width: 32, .. } => CV::I(*bits),
            Type::Float { width: 32 } => CV::F32(f32::from_bits(*bits)),
            _ => return None,
        },
        ConstKind::Bits64(bits) => match l.types.get(&c.ty)? {
            Type::Float { width: 64 } => CV::F64(f64::from_bits(*bits)),
            _ => return None,
        },
        ConstKind::Composite(ids) => CV::Comp(ids.iter().map(|i| read(l, *i)).collect::<Option<_>>()?),
        ConstKind::Null => zero(l, c.ty)?,
        ConstKind::Undef | ConstKind::SpecOp(_) => return None,
    })
}

/// Component types of a composite type, in order.
fn member_types(l: &Lifted, ty: u32) -> Option<Vec<u32>> {
    Some(match l.types.get(&ty)? {
        Type::Vector { elem, count } => vec![*elem; *count as usize],
        Type::Matrix { column, columns } => vec![*column; *columns as usize],
        Type::Array { elem, len } => vec![*elem; l.array_len(*len).ok()? as usize],
        Type::Struct { members } => members.clone(),
        _ => return None,
    })
}

/// Returns the id of a constant of type `ty` with value `v`, reusing an existing
/// (non-specialization) constant when one has the same bits, else appending a new
/// `OpConstant`/`OpConstantComposite` with a fresh id. `None` when the type is unsupported.
pub fn intern(l: &mut Lifted, ty: u32, v: &CV) -> Option<u32> {
    // Existing constant?
    for (id, c) in &l.constants {
        if c.ty == ty && !c.spec && !matches!(c.kind, ConstKind::Null | ConstKind::Undef | ConstKind::SpecOp(_)) {
            if let Some(cv) = read(l, *id) {
                if cv.bits_eq(v) {
                    return Some(*id);
                }
            }
        }
    }
    let (opcode, operands): (Op, Vec<Operand>) = match v {
        CV::Bool(true) => (Op::ConstantTrue, vec![]),
        CV::Bool(false) => (Op::ConstantFalse, vec![]),
        CV::I(x) => (Op::Constant, vec![Operand::LiteralBit32(*x)]),
        CV::F32(x) => (Op::Constant, vec![Operand::LiteralBit32(x.to_bits())]),
        CV::F64(x) => (Op::Constant, vec![Operand::LiteralBit64(x.to_bits())]),
        CV::Comp(items) => {
            let mts = member_types(l, ty)?;
            if mts.len() != items.len() {
                return None;
            }
            let mut ids = Vec::with_capacity(items.len());
            for (mt, item) in mts.iter().zip(items) {
                ids.push(Operand::IdRef(intern(l, *mt, item)?));
            }
            (Op::ConstantComposite, ids)
        }
    };
    // Type check for scalars.
    match (v, l.types.get(&ty)?) {
        (CV::Bool(_), Type::Bool) | (CV::I(_), Type::Int { width: 32, .. }) | (CV::F32(_), Type::Float { width: 32 }) | (CV::F64(_), Type::Float { width: 64 }) => {}
        (CV::Comp(_), _) => {}
        _ => return None,
    }
    let id = super::fresh_id(l);
    let inst = dr::Instruction::new(opcode, Some(ty), Some(id), operands);
    l.module.types_global_values.push(inst);
    // Keep the side tables usable for further interning before the next reanalyze.
    let kind = match v {
        CV::Bool(b) => ConstKind::Bool(*b),
        CV::I(x) => ConstKind::Bits32(*x),
        CV::F32(x) => ConstKind::Bits32(x.to_bits()),
        CV::F64(x) => ConstKind::Bits64(x.to_bits()),
        CV::Comp(_) => ConstKind::Composite(l.module.types_global_values.last().unwrap().operands.iter().filter_map(|o| o.id_ref_any()).collect()),
    };
    l.constants.insert(id, Constant { ty, kind, spec: false });
    l.result_types.insert(id, ty);
    l.defs.insert(id, crate::lift::Site::Global(l.module.types_global_values.len() - 1));
    Some(id)
}

/// A splat constant of a scalar-or-vector type with every component `x` (f32 or f64 per the type).
pub fn splat_f(l: &mut Lifted, ty: u32, x: f64) -> Option<u32> {
    let (elem, n) = super::vector_shape(l, ty)?;
    let s = match l.types.get(&elem)? {
        Type::Float { width: 32 } => CV::F32(x as f32),
        Type::Float { width: 64 } => CV::F64(x),
        _ => return None,
    };
    let v = if n == 1 { s } else { CV::Comp(vec![s; n as usize]) };
    intern(l, ty, &v)
}

// ---- evaluation ----------------------------------------------------------------------------

/// A fold result: the value and its class.
pub struct Folded {
    pub value: CV,
    pub class: Class,
}

struct Acc {
    class: Class,
}

impl Acc {
    fn ulp(&mut self) {
        self.class = Class::Ulp;
    }
    /// Records an f32 operation: exact iff the f32 result equals the f64 evaluation.
    fn f32_op(&mut self, r32: f32, r64: f64) -> CV {
        let same = (r32 as f64).to_bits() == r64.to_bits() || (r32.is_nan() && r64.is_nan());
        if !same {
            self.ulp();
        }
        CV::F32(r32)
    }
}

fn elem_type(l: &Lifted, ty: u32) -> Option<u32> {
    super::vector_shape(l, ty).map(|(e, _)| e)
}

fn zip2(a: &CV, b: &CV, f: &mut dyn FnMut(&CV, &CV) -> Option<CV>) -> Option<CV> {
    match (a, b) {
        (CV::Comp(x), CV::Comp(y)) if x.len() == y.len() => {
            Some(CV::Comp(x.iter().zip(y).map(|(p, q)| f(p, q)).collect::<Option<_>>()?))
        }
        (CV::Comp(_), _) | (_, CV::Comp(_)) => None,
        (x, y) => f(x, y),
    }
}

fn zip3(a: &CV, b: &CV, c: &CV, f: &mut dyn FnMut(&CV, &CV, &CV) -> Option<CV>) -> Option<CV> {
    match (a, b, c) {
        (CV::Comp(x), CV::Comp(y), CV::Comp(z)) if x.len() == y.len() && y.len() == z.len() => Some(CV::Comp(
            x.iter().zip(y).zip(z).map(|((p, q), r)| f(p, q, r)).collect::<Option<_>>()?,
        )),
        (CV::Comp(_), _, _) | (_, CV::Comp(_), _) | (_, _, CV::Comp(_)) => None,
        (x, y, z) => f(x, y, z),
    }
}

fn map1(a: &CV, f: &mut dyn FnMut(&CV) -> Option<CV>) -> Option<CV> {
    match a {
        CV::Comp(x) => Some(CV::Comp(x.iter().map(|p| f(p)).collect::<Option<_>>()?)),
        x => f(x),
    }
}

/// Float binary op with an f32 and an f64 implementation.
fn fbin(acc: &mut Acc, a: &CV, b: &CV, f32op: fn(f32, f32) -> f32, f64op: fn(f64, f64) -> f64) -> Option<CV> {
    zip2(a, b, &mut |x, y| match (x, y) {
        (CV::F32(p), CV::F32(q)) => Some(acc.f32_op(f32op(*p, *q), f64op(*p as f64, *q as f64))),
        (CV::F64(p), CV::F64(q)) => Some(CV::F64(f64op(*p, *q))),
        _ => None,
    })
}

fn fun(acc: &mut Acc, a: &CV, f32op: fn(f32) -> f32, f64op: fn(f64) -> f64) -> Option<CV> {
    map1(a, &mut |x| match x {
        CV::F32(p) => Some(acc.f32_op(f32op(*p), f64op(*p as f64))),
        CV::F64(p) => Some(CV::F64(f64op(*p))),
        _ => None,
    })
}

/// Transcendental (always `ulp`), computed with Rust's f32/f64 functions.
fn ftrans1(acc: &mut Acc, a: &CV, f32op: fn(f32) -> f32, f64op: fn(f64) -> f64) -> Option<CV> {
    acc.ulp();
    map1(a, &mut |x| match x {
        CV::F32(p) => Some(CV::F32(f32op(*p))),
        CV::F64(p) => Some(CV::F64(f64op(*p))),
        _ => None,
    })
}

fn ftrans2(acc: &mut Acc, a: &CV, b: &CV, f32op: fn(f32, f32) -> f32, f64op: fn(f64, f64) -> f64) -> Option<CV> {
    acc.ulp();
    zip2(a, b, &mut |x, y| match (x, y) {
        (CV::F32(p), CV::F32(q)) => Some(CV::F32(f32op(*p, *q))),
        (CV::F64(p), CV::F64(q)) => Some(CV::F64(f64op(*p, *q))),
        _ => None,
    })
}

fn fcmp(a: &CV, b: &CV, f: fn(f64, f64) -> bool) -> Option<CV> {
    zip2(a, b, &mut |x, y| Some(CV::Bool(f(x.as_f64()?, y.as_f64()?))))
}

fn ibin(a: &CV, b: &CV, f: &mut dyn FnMut(u32, u32) -> Option<u32>) -> Option<CV> {
    zip2(a, b, &mut |x, y| Some(CV::I(f(x.as_i()?, y.as_i()?)?)))
}

fn icmp(a: &CV, b: &CV, f: fn(u32, u32) -> bool) -> Option<CV> {
    zip2(a, b, &mut |x, y| Some(CV::Bool(f(x.as_i()?, y.as_i()?))))
}

fn lbin(a: &CV, b: &CV, f: fn(bool, bool) -> bool) -> Option<CV> {
    zip2(a, b, &mut |x, y| Some(CV::Bool(f(x.as_bool()?, y.as_bool()?))))
}

fn sign32(x: f32) -> f32 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        0.0
    }
}
fn sign64(x: f64) -> f64 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        0.0
    }
}

/// `dot`: products then a left-to-right sum (the interpreter's definition).
fn dot_f(acc: &mut Acc, a: &CV, b: &CV) -> Option<CV> {
    acc.ulp();
    let (x, y) = (a.comps(), b.comps());
    if x.len() != y.len() || x.is_empty() {
        return None;
    }
    match x[0] {
        CV::F32(_) => {
            let mut s = 0f32;
            for (i, (p, q)) in x.iter().zip(&y).enumerate() {
                let (CV::F32(p), CV::F32(q)) = (p, q) else { return None };
                let m = p * q;
                s = if i == 0 { m } else { s + m };
            }
            Some(CV::F32(s))
        }
        CV::F64(_) => {
            let mut s = 0f64;
            for (i, (p, q)) in x.iter().zip(&y).enumerate() {
                let (CV::F64(p), CV::F64(q)) = (p, q) else { return None };
                let m = p * q;
                s = if i == 0 { m } else { s + m };
            }
            Some(CV::F64(s))
        }
        _ => None,
    }
}

/// Evaluates `inst` given its constant operands (`args[i]` is the value of the i-th id operand,
/// ext-inst set excluded). `None` when the instruction is not foldable.
pub fn eval(l: &Lifted, inst: &dr::Instruction, args: &[CV]) -> Option<Folded> {
    let mut acc = Acc { class: Class::Exact };
    let rty = inst.result_type?;
    let a = |i: usize| args.get(i);
    let value = if let Some(g) = glsl_op(l, inst) {
        eval_glsl(g, args, &mut acc)?
    } else {
        use Op::*;
        match inst.class.opcode {
            FNegate => fun(&mut acc, a(0)?, |x| -x, |x| -x)?,
            FAdd => fbin(&mut acc, a(0)?, a(1)?, |x, y| x + y, |x, y| x + y)?,
            FSub => fbin(&mut acc, a(0)?, a(1)?, |x, y| x - y, |x, y| x - y)?,
            FMul => fbin(&mut acc, a(0)?, a(1)?, |x, y| x * y, |x, y| x * y)?,
            FDiv => fbin(&mut acc, a(0)?, a(1)?, |x, y| x / y, |x, y| x / y)?,
            FRem => fbin(&mut acc, a(0)?, a(1)?, |x, y| x % y, |x, y| x % y)?,
            FMod => fbin(&mut acc, a(0)?, a(1)?, |x, y| x - y * (x / y).floor(), |x, y| x - y * (x / y).floor())?,
            VectorTimesScalar => {
                let (v, s) = (a(0)?, a(1)?);
                map1(v, &mut |x| match (x, s) {
                    (CV::F32(p), CV::F32(q)) => Some(acc.f32_op(p * q, (*p as f64) * (*q as f64))),
                    (CV::F64(p), CV::F64(q)) => Some(CV::F64(p * q)),
                    _ => None,
                })?
            }
            Dot => dot_f(&mut acc, a(0)?, a(1)?)?,
            SNegate => map1(a(0)?, &mut |x| Some(CV::I((x.as_i()? as i32).wrapping_neg() as u32)))?,
            IAdd => ibin(a(0)?, a(1)?, &mut |x, y| Some(x.wrapping_add(y)))?,
            ISub => ibin(a(0)?, a(1)?, &mut |x, y| Some(x.wrapping_sub(y)))?,
            IMul => ibin(a(0)?, a(1)?, &mut |x, y| Some(x.wrapping_mul(y)))?,
            SDiv => ibin(a(0)?, a(1)?, &mut |x, y| (x as i32).checked_div(y as i32).map(|v| v as u32))?,
            UDiv => ibin(a(0)?, a(1)?, &mut |x, y| x.checked_div(y))?,
            SRem => ibin(a(0)?, a(1)?, &mut |x, y| (x as i32).checked_rem(y as i32).map(|v| v as u32))?,
            SMod => ibin(a(0)?, a(1)?, &mut |x, y| {
                let (x, y) = (x as i32, y as i32);
                let r = x.checked_rem(y)?;
                Some(if r != 0 && ((r < 0) != (y < 0)) { r.wrapping_add(y) } else { r } as u32)
            })?,
            UMod => ibin(a(0)?, a(1)?, &mut |x, y| x.checked_rem(y))?,
            BitwiseAnd => ibin(a(0)?, a(1)?, &mut |x, y| Some(x & y))?,
            BitwiseOr => ibin(a(0)?, a(1)?, &mut |x, y| Some(x | y))?,
            BitwiseXor => ibin(a(0)?, a(1)?, &mut |x, y| Some(x ^ y))?,
            Not => map1(a(0)?, &mut |x| Some(CV::I(!x.as_i()?)))?,
            ShiftLeftLogical => ibin(a(0)?, a(1)?, &mut |x, s| if s < 32 { Some(x << s) } else { None })?,
            ShiftRightLogical => ibin(a(0)?, a(1)?, &mut |x, s| if s < 32 { Some(x >> s) } else { None })?,
            ShiftRightArithmetic => ibin(a(0)?, a(1)?, &mut |x, s| if s < 32 { Some(((x as i32) >> s) as u32) } else { None })?,
            FOrdEqual => fcmp(a(0)?, a(1)?, |x, y| x == y)?,
            FOrdNotEqual => fcmp(a(0)?, a(1)?, |x, y| x != y && !x.is_nan() && !y.is_nan())?,
            FOrdLessThan => fcmp(a(0)?, a(1)?, |x, y| x < y)?,
            FOrdGreaterThan => fcmp(a(0)?, a(1)?, |x, y| x > y)?,
            FOrdLessThanEqual => fcmp(a(0)?, a(1)?, |x, y| x <= y)?,
            FOrdGreaterThanEqual => fcmp(a(0)?, a(1)?, |x, y| x >= y)?,
            FUnordEqual => fcmp(a(0)?, a(1)?, |x, y| x == y || x.is_nan() || y.is_nan())?,
            FUnordNotEqual => fcmp(a(0)?, a(1)?, |x, y| x != y)?,
            FUnordLessThan => fcmp(a(0)?, a(1)?, |x, y| x < y || x.is_nan() || y.is_nan())?,
            FUnordGreaterThan => fcmp(a(0)?, a(1)?, |x, y| x > y || x.is_nan() || y.is_nan())?,
            FUnordLessThanEqual => fcmp(a(0)?, a(1)?, |x, y| x <= y || x.is_nan() || y.is_nan())?,
            FUnordGreaterThanEqual => fcmp(a(0)?, a(1)?, |x, y| x >= y || x.is_nan() || y.is_nan())?,
            IEqual => icmp(a(0)?, a(1)?, |x, y| x == y)?,
            INotEqual => icmp(a(0)?, a(1)?, |x, y| x != y)?,
            ULessThan => icmp(a(0)?, a(1)?, |x, y| x < y)?,
            ULessThanEqual => icmp(a(0)?, a(1)?, |x, y| x <= y)?,
            UGreaterThan => icmp(a(0)?, a(1)?, |x, y| x > y)?,
            UGreaterThanEqual => icmp(a(0)?, a(1)?, |x, y| x >= y)?,
            SLessThan => icmp(a(0)?, a(1)?, |x, y| (x as i32) < (y as i32))?,
            SLessThanEqual => icmp(a(0)?, a(1)?, |x, y| (x as i32) <= (y as i32))?,
            SGreaterThan => icmp(a(0)?, a(1)?, |x, y| (x as i32) > (y as i32))?,
            SGreaterThanEqual => icmp(a(0)?, a(1)?, |x, y| (x as i32) >= (y as i32))?,
            LogicalAnd => lbin(a(0)?, a(1)?, |x, y| x && y)?,
            LogicalOr => lbin(a(0)?, a(1)?, |x, y| x || y)?,
            LogicalEqual => lbin(a(0)?, a(1)?, |x, y| x == y)?,
            LogicalNotEqual => lbin(a(0)?, a(1)?, |x, y| x != y)?,
            LogicalNot => map1(a(0)?, &mut |x| Some(CV::Bool(!x.as_bool()?)))?,
            Any => CV::Bool(a(0)?.comps().iter().any(|x| x.as_bool() == Some(true))),
            All => CV::Bool(a(0)?.comps().iter().all(|x| x.as_bool() == Some(true))),
            IsNan => map1(a(0)?, &mut |x| Some(CV::Bool(x.as_f64()?.is_nan())))?,
            IsInf => map1(a(0)?, &mut |x| Some(CV::Bool(x.as_f64()?.is_infinite())))?,
            Select => {
                let (c, x, y) = (a(0)?, a(1)?, a(2)?);
                match c {
                    CV::Bool(b) => {
                        if *b {
                            x.clone()
                        } else {
                            y.clone()
                        }
                    }
                    CV::Comp(_) => zip3(c, x, y, &mut |c, x, y| Some(if c.as_bool()? { x.clone() } else { y.clone() }))?,
                    _ => return None,
                }
            }
            CopyObject => a(0)?.clone(),
            CompositeConstruct => {
                // A vector may be built from smaller vectors and scalars: flatten.
                let is_vec = matches!(l.types.get(&rty)?, Type::Vector { .. });
                let mut items = Vec::new();
                for v in args {
                    match v {
                        CV::Comp(inner) if is_vec => items.extend(inner.iter().cloned()),
                        v => items.push(v.clone()),
                    }
                }
                CV::Comp(items)
            }
            CompositeExtract => {
                let mut cur = a(0)?.clone();
                for o in inst.operands.iter().skip(1) {
                    let Operand::LiteralBit32(i) = o else { return None };
                    cur = match cur {
                        CV::Comp(v) => v.get(*i as usize)?.clone(),
                        _ => return None,
                    };
                }
                cur
            }
            CompositeInsert => {
                let obj = a(0)?.clone();
                let mut comp = a(1)?.clone();
                let idx: Vec<u32> = inst.operands.iter().skip(2).map(|o| match o {
                    Operand::LiteralBit32(i) => Some(*i),
                    _ => None,
                }).collect::<Option<_>>()?;
                fn ins(c: &mut CV, idx: &[u32], obj: CV) -> Option<()> {
                    match idx.split_first() {
                        None => {
                            *c = obj;
                            Some(())
                        }
                        Some((i, rest)) => match c {
                            CV::Comp(v) => ins(v.get_mut(*i as usize)?, rest, obj),
                            _ => None,
                        },
                    }
                }
                ins(&mut comp, &idx, obj)?;
                comp
            }
            VectorShuffle => {
                let (x, y) = (a(0)?.comps(), a(1)?.comps());
                let mut out = Vec::new();
                for o in inst.operands.iter().skip(2) {
                    let Operand::LiteralBit32(i) = o else { return None };
                    let i = *i as usize;
                    if i == 0xFFFF_FFFF {
                        return None;
                    }
                    out.push(if i < x.len() { x.get(i)?.clone() } else { y.get(i - x.len())?.clone() });
                }
                CV::Comp(out)
            }
            ConvertFToS => map1(a(0)?, &mut |x| {
                let f = x.as_f64()?;
                if !(f > -2147483649.0 && f < 2147483648.0) {
                    return None;
                }
                Some(CV::I(f.trunc() as i32 as u32))
            })?,
            ConvertFToU => map1(a(0)?, &mut |x| {
                let f = x.as_f64()?;
                if !(f > -1.0 && f < 4294967296.0) {
                    return None;
                }
                Some(CV::I(f.trunc() as u32))
            })?,
            ConvertSToF | ConvertUToF => {
                let signed = inst.class.opcode == ConvertSToF;
                let et = elem_type(l, rty)?;
                map1(a(0)?, &mut |x| {
                    let i = x.as_i()?;
                    let f = if signed { i as i32 as f64 } else { i as f64 };
                    Some(match l.types.get(&et)? {
                        Type::Float { width: 32 } => acc.f32_op(if signed { i as i32 as f32 } else { i as f32 }, f),
                        Type::Float { width: 64 } => CV::F64(f),
                        _ => return None,
                    })
                })?
            }
            FConvert => {
                let et = elem_type(l, rty)?;
                map1(a(0)?, &mut |x| {
                    let f = x.as_f64()?;
                    Some(match l.types.get(&et)? {
                        Type::Float { width: 32 } => acc.f32_op(f as f32, f),
                        Type::Float { width: 64 } => CV::F64(f),
                        _ => return None,
                    })
                })?
            }
            Bitcast => {
                let et = elem_type(l, rty)?;
                map1(a(0)?, &mut |x| {
                    let bits = match x {
                        CV::I(b) => *b,
                        CV::F32(f) => f.to_bits(),
                        _ => return None,
                    };
                    Some(match l.types.get(&et)? {
                        Type::Int { width: 32, .. } => CV::I(bits),
                        Type::Float { width: 32 } => CV::F32(f32::from_bits(bits)),
                        _ => return None,
                    })
                })?
            }
            _ => return None,
        }
    };
    Some(Folded { value, class: acc.class })
}

fn eval_glsl(g: GLOp, args: &[CV], acc: &mut Acc) -> Option<CV> {
    let a = |i: usize| args.get(i);
    Some(match g {
        GLOp::Round => fun(acc, a(0)?, f32::round, f64::round)?,
        GLOp::RoundEven => fun(acc, a(0)?, f32::round_ties_even, f64::round_ties_even)?,
        GLOp::Trunc => fun(acc, a(0)?, f32::trunc, f64::trunc)?,
        GLOp::FAbs => fun(acc, a(0)?, f32::abs, f64::abs)?,
        GLOp::FSign => fun(acc, a(0)?, sign32, sign64)?,
        GLOp::Floor => fun(acc, a(0)?, f32::floor, f64::floor)?,
        GLOp::Ceil => fun(acc, a(0)?, f32::ceil, f64::ceil)?,
        GLOp::Fract => fun(acc, a(0)?, |x| x - x.floor(), |x| x - x.floor())?,
        GLOp::Radians => ftrans1(acc, a(0)?, |x| x * (std::f64::consts::PI / 180.0) as f32, |x| x * (std::f64::consts::PI / 180.0))?,
        GLOp::Degrees => ftrans1(acc, a(0)?, |x| x * (180.0 / std::f64::consts::PI) as f32, |x| x * (180.0 / std::f64::consts::PI))?,
        GLOp::Sin => ftrans1(acc, a(0)?, f32::sin, f64::sin)?,
        GLOp::Cos => ftrans1(acc, a(0)?, f32::cos, f64::cos)?,
        GLOp::Tan => ftrans1(acc, a(0)?, f32::tan, f64::tan)?,
        GLOp::Asin => ftrans1(acc, a(0)?, f32::asin, f64::asin)?,
        GLOp::Acos => ftrans1(acc, a(0)?, f32::acos, f64::acos)?,
        GLOp::Atan => ftrans1(acc, a(0)?, f32::atan, f64::atan)?,
        GLOp::Sinh => ftrans1(acc, a(0)?, f32::sinh, f64::sinh)?,
        GLOp::Cosh => ftrans1(acc, a(0)?, f32::cosh, f64::cosh)?,
        GLOp::Tanh => ftrans1(acc, a(0)?, f32::tanh, f64::tanh)?,
        GLOp::Asinh => ftrans1(acc, a(0)?, f32::asinh, f64::asinh)?,
        GLOp::Acosh => ftrans1(acc, a(0)?, f32::acosh, f64::acosh)?,
        GLOp::Atanh => ftrans1(acc, a(0)?, f32::atanh, f64::atanh)?,
        GLOp::Atan2 => ftrans2(acc, a(0)?, a(1)?, f32::atan2, f64::atan2)?,
        GLOp::Pow => ftrans2(acc, a(0)?, a(1)?, f32::powf, f64::powf)?,
        GLOp::Exp => ftrans1(acc, a(0)?, f32::exp, f64::exp)?,
        GLOp::Log => ftrans1(acc, a(0)?, f32::ln, f64::ln)?,
        GLOp::Exp2 => ftrans1(acc, a(0)?, f32::exp2, f64::exp2)?,
        GLOp::Log2 => ftrans1(acc, a(0)?, f32::log2, f64::log2)?,
        GLOp::Sqrt => ftrans1(acc, a(0)?, f32::sqrt, f64::sqrt)?,
        GLOp::InverseSqrt => ftrans1(acc, a(0)?, |x| 1.0 / x.sqrt(), |x| 1.0 / x.sqrt())?,
        GLOp::FMin | GLOp::NMin => fun2_minmax(acc, a(0)?, a(1)?, true)?,
        GLOp::FMax | GLOp::NMax => fun2_minmax(acc, a(0)?, a(1)?, false)?,
        GLOp::FClamp | GLOp::NClamp => {
            let lo = fun2_minmax(acc, a(0)?, a(1)?, false)?;
            fun2_minmax(acc, &lo, a(2)?, true)?
        }
        GLOp::SMin => ibin(a(0)?, a(1)?, &mut |x, y| Some((x as i32).min(y as i32) as u32))?,
        GLOp::SMax => ibin(a(0)?, a(1)?, &mut |x, y| Some((x as i32).max(y as i32) as u32))?,
        GLOp::UMin => ibin(a(0)?, a(1)?, &mut |x, y| Some(x.min(y)))?,
        GLOp::UMax => ibin(a(0)?, a(1)?, &mut |x, y| Some(x.max(y)))?,
        GLOp::SClamp => zip3(a(0)?, a(1)?, a(2)?, &mut |x, lo, hi| {
            Some(CV::I((x.as_i()? as i32).max(lo.as_i()? as i32).min(hi.as_i()? as i32) as u32))
        })?,
        GLOp::UClamp => zip3(a(0)?, a(1)?, a(2)?, &mut |x, lo, hi| Some(CV::I(x.as_i()?.max(lo.as_i()?).min(hi.as_i()?))))?,
        GLOp::SAbs => map1(a(0)?, &mut |x| Some(CV::I((x.as_i()? as i32).wrapping_abs() as u32)))?,
        GLOp::SSign => map1(a(0)?, &mut |x| Some(CV::I((x.as_i()? as i32).signum() as u32)))?,
        GLOp::Step => zip2(a(0)?, a(1)?, &mut |e, x| match (e, x) {
            (CV::F32(e), CV::F32(x)) => Some(CV::F32(if x < e { 0.0 } else { 1.0 })),
            (CV::F64(e), CV::F64(x)) => Some(CV::F64(if x < e { 0.0 } else { 1.0 })),
            _ => None,
        })?,
        GLOp::Fma => zip3(a(0)?, a(1)?, a(2)?, &mut |x, y, z| match (x, y, z) {
            (CV::F32(x), CV::F32(y), CV::F32(z)) => Some(acc.f32_op(x.mul_add(*y, *z), (*x as f64).mul_add(*y as f64, *z as f64))),
            (CV::F64(x), CV::F64(y), CV::F64(z)) => Some(CV::F64(x.mul_add(*y, *z))),
            _ => None,
        })?,
        GLOp::FMix => {
            acc.ulp();
            zip3(a(0)?, a(1)?, a(2)?, &mut |x, y, t| match (x, y, t) {
                (CV::F32(x), CV::F32(y), CV::F32(t)) => Some(CV::F32(x * (1.0 - t) + y * t)),
                (CV::F64(x), CV::F64(y), CV::F64(t)) => Some(CV::F64(x * (1.0 - t) + y * t)),
                _ => None,
            })?
        }
        GLOp::SmoothStep => {
            acc.ulp();
            zip3(a(0)?, a(1)?, a(2)?, &mut |e0, e1, x| match (e0, e1, x) {
                (CV::F32(e0), CV::F32(e1), CV::F32(x)) => {
                    let t = ((x - e0) / (e1 - e0)).max(0.0).min(1.0);
                    Some(CV::F32(t * t * (3.0 - 2.0 * t)))
                }
                (CV::F64(e0), CV::F64(e1), CV::F64(x)) => {
                    let t = ((x - e0) / (e1 - e0)).max(0.0).min(1.0);
                    Some(CV::F64(t * t * (3.0 - 2.0 * t)))
                }
                _ => None,
            })?
        }
        GLOp::Length => {
            let d = dot_f(acc, a(0)?, a(0)?)?;
            ftrans1(acc, &d, f32::sqrt, f64::sqrt)?
        }
        GLOp::Distance => {
            let d = fbin(acc, a(0)?, a(1)?, |x, y| x - y, |x, y| x - y)?;
            let dd = dot_f(acc, &d, &d)?;
            ftrans1(acc, &dd, f32::sqrt, f64::sqrt)?
        }
        GLOp::Normalize => {
            let v = a(0)?;
            let d = dot_f(acc, v, v)?;
            let inv = ftrans1(acc, &d, |x| 1.0 / x.sqrt(), |x| 1.0 / x.sqrt())?;
            map1(v, &mut |x| match (x, &inv) {
                (CV::F32(p), CV::F32(q)) => Some(CV::F32(p * q)),
                (CV::F64(p), CV::F64(q)) => Some(CV::F64(p * q)),
                _ => None,
            })?
        }
        GLOp::Cross => {
            acc.ulp();
            let (x, y) = (a(0)?.comps(), a(1)?.comps());
            if x.len() != 3 || y.len() != 3 {
                return None;
            }
            let f: Vec<f64> = x.iter().chain(&y).map(|c| c.as_f64()).collect::<Option<_>>()?;
            let (x0, x1, x2, y0, y1, y2) = (f[0], f[1], f[2], f[3], f[4], f[5]);
            let r = [x1 * y2 - y1 * x2, x2 * y0 - y2 * x0, x0 * y1 - y0 * x1];
            match x[0] {
                CV::F32(_) => {
                    let g: Vec<f32> = f.iter().map(|v| *v as f32).collect();
                    let r32 = [g[1] * g[5] - g[4] * g[2], g[2] * g[3] - g[5] * g[0], g[0] * g[4] - g[3] * g[1]];
                    CV::Comp(r32.iter().map(|v| CV::F32(*v)).collect())
                }
                _ => CV::Comp(r.iter().map(|v| CV::F64(*v)).collect()),
            }
        }
        _ => return None,
    })
}

/// `min`/`max` with IEEE minNum/maxNum semantics (Rust `f32::min`, as the interpreter uses).
fn fun2_minmax(acc: &mut Acc, a: &CV, b: &CV, is_min: bool) -> Option<CV> {
    if is_min {
        fbin(acc, a, b, f32::min, f64::min)
    } else {
        fbin(acc, a, b, f32::max, f64::max)
    }
}
