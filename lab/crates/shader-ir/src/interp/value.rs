//! Runtime values and numeric helpers.

use anyhow::{anyhow, bail, Result};
use std::ops::{Add, Div, Mul, Neg, Rem, Sub};

/// Numeric mode of the interpreter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Every float operation is evaluated in f64 (inputs widened).
    F64,
    /// IEEE f32 for 32-bit float types, f16/f64 for 16/64-bit types: exactly what the SPIR-V says.
    F32,
    /// Prediction of a demoted shader: every float-typed instruction is computed in f32 and its
    /// result rounded to f16 (round to nearest even) via `half::f16`.
    F16,
}

impl std::str::FromStr for Mode {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        Ok(match s {
            "f64" => Mode::F64,
            "f32" => Mode::F32,
            "f16" => Mode::F16,
            other => bail!("unknown mode {other:?}; want f64, f32 or f16"),
        })
    }
}

/// Effective precision of one float operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prec {
    P16,
    P32,
    P64,
}

impl Mode {
    /// Precision used for an operation whose float type has `width` bits.
    #[inline]
    pub fn prec(self, width: u32) -> Prec {
        match self {
            Mode::F64 => Prec::P64,
            Mode::F32 => match width {
                64 => Prec::P64,
                16 => Prec::P16,
                _ => Prec::P32,
            },
            Mode::F16 => match width {
                64 => Prec::P64,
                _ => Prec::P16,
            },
        }
    }
}

/// A pointer: the root variable id plus an access path of composite indices.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ptr {
    pub root: u32,
    pub path: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Undef,
    Bool(bool),
    I32(i32),
    U32(u32),
    /// Any float type; the stored f64 is always exactly representable at the type's precision.
    F(f64),
    /// Vector (scalars), matrix (column vectors), struct (members) or array (elements).
    V(Vec<Value>),
    Ptr(Ptr),
    /// Index into the program's image table.
    Image(usize),
    Sampler,
    SampledImage(usize),
}

impl Value {
    #[inline]
    pub fn as_f(&self) -> Result<f64> {
        match self {
            Value::F(x) => Ok(*x),
            other => Err(anyhow!("expected a float scalar, got {}", other.describe())),
        }
    }
    #[inline]
    pub fn as_bool(&self) -> Result<bool> {
        match self {
            Value::Bool(b) => Ok(*b),
            other => Err(anyhow!("expected a bool scalar, got {}", other.describe())),
        }
    }
    /// Bit pattern of a 32-bit integer of either signedness.
    #[inline]
    pub fn as_bits(&self) -> Result<u32> {
        match self {
            Value::I32(x) => Ok(*x as u32),
            Value::U32(x) => Ok(*x),
            other => Err(anyhow!("expected an integer scalar, got {}", other.describe())),
        }
    }
    #[inline]
    pub fn as_index(&self) -> Result<usize> {
        match self {
            Value::I32(x) if *x >= 0 => Ok(*x as usize),
            Value::I32(x) => Err(anyhow!("negative index {x}")),
            Value::U32(x) => Ok(*x as usize),
            other => Err(anyhow!("expected an integer index, got {}", other.describe())),
        }
    }
    #[inline]
    pub fn as_vec(&self) -> Result<&[Value]> {
        match self {
            Value::V(v) => Ok(v),
            other => Err(anyhow!("expected a composite, got {}", other.describe())),
        }
    }
    /// Scalars behave as one-element slices; vectors as their components.
    #[inline]
    pub fn comps(&self) -> &[Value] {
        match self {
            Value::V(v) => v,
            s => std::slice::from_ref(s),
        }
    }
    pub fn describe(&self) -> String {
        match self {
            Value::Undef => "undef".into(),
            Value::Bool(_) => "bool".into(),
            Value::I32(_) => "int".into(),
            Value::U32(_) => "uint".into(),
            Value::F(_) => "float".into(),
            Value::V(v) => format!("composite[{}]", v.len()),
            Value::Ptr(p) => format!("pointer(%{} {:?})", p.root, p.path),
            Value::Image(_) => "image".into(),
            Value::Sampler => "sampler".into(),
            Value::SampledImage(_) => "sampled image".into(),
        }
    }
    /// Reads all float scalar leaves of a vector/scalar.
    pub fn floats(&self) -> Result<Vec<f64>> {
        self.comps().iter().map(|c| c.as_f()).collect()
    }
}

/// Applies `f` to every scalar leaf of a value (vectors and matrices recurse).
pub fn map1(v: &Value, f: &mut dyn FnMut(&Value) -> Result<Value>) -> Result<Value> {
    match v {
        Value::V(xs) => Ok(Value::V(xs.iter().map(|x| map1(x, f)).collect::<Result<_>>()?)),
        s => f(s),
    }
}

pub fn map2(a: &Value, b: &Value, f: &mut dyn FnMut(&Value, &Value) -> Result<Value>) -> Result<Value> {
    match (a, b) {
        (Value::V(xs), Value::V(ys)) => {
            if xs.len() != ys.len() {
                bail!("component count mismatch {} vs {}", xs.len(), ys.len());
            }
            Ok(Value::V(xs.iter().zip(ys).map(|(x, y)| map2(x, y, f)).collect::<Result<_>>()?))
        }
        (Value::V(_), _) | (_, Value::V(_)) => bail!("mixed composite and scalar operands"),
        (x, y) => f(x, y),
    }
}

pub fn map3(
    a: &Value,
    b: &Value,
    c: &Value,
    f: &mut dyn FnMut(&Value, &Value, &Value) -> Result<Value>,
) -> Result<Value> {
    match (a, b, c) {
        (Value::V(xs), Value::V(ys), Value::V(zs)) => {
            if xs.len() != ys.len() || xs.len() != zs.len() {
                bail!("component count mismatch {} / {} / {}", xs.len(), ys.len(), zs.len());
            }
            Ok(Value::V(
                xs.iter().zip(ys).zip(zs).map(|((x, y), z)| map3(x, y, z, f)).collect::<Result<_>>()?,
            ))
        }
        (Value::V(_), _, _) | (_, Value::V(_), _) | (_, _, Value::V(_)) => bail!("mixed composite and scalar operands"),
        (x, y, z) => f(x, y, z),
    }
}

/// Rounds an f32 result to f16 (RNE) and widens it back.
#[inline]
pub fn r16(x: f32) -> f64 {
    half::f16::from_f32(x).to_f64()
}

/// Float arithmetic at one precision.
#[derive(Clone, Copy, Debug)]
pub struct Fp {
    pub prec: Prec,
}

macro_rules! fp1 {
    ($self:expr, $x:expr, $m:ident) => {
        match $self.prec {
            Prec::P64 => f64::$m($x),
            Prec::P32 => f32::$m($x as f32) as f64,
            Prec::P16 => r16(f32::$m($x as f32)),
        }
    };
}
macro_rules! fp2 {
    ($self:expr, $x:expr, $y:expr, $m:ident) => {
        match $self.prec {
            Prec::P64 => f64::$m($x, $y),
            Prec::P32 => f32::$m($x as f32, $y as f32) as f64,
            Prec::P16 => r16(f32::$m($x as f32, $y as f32)),
        }
    };
}

impl Fp {
    /// Rounds a value to this precision.
    #[inline]
    pub fn round(self, x: f64) -> f64 {
        match self.prec {
            Prec::P64 => x,
            Prec::P32 => x as f32 as f64,
            Prec::P16 => r16(x as f32),
        }
    }
    #[inline]
    pub fn add(self, x: f64, y: f64) -> f64 {
        fp2!(self, x, y, add)
    }
    #[inline]
    pub fn sub(self, x: f64, y: f64) -> f64 {
        fp2!(self, x, y, sub)
    }
    #[inline]
    pub fn mul(self, x: f64, y: f64) -> f64 {
        fp2!(self, x, y, mul)
    }
    #[inline]
    pub fn div(self, x: f64, y: f64) -> f64 {
        fp2!(self, x, y, div)
    }
    /// `OpFRem`: sign of the dividend (Rust `%`).
    #[inline]
    pub fn rem(self, x: f64, y: f64) -> f64 {
        fp2!(self, x, y, rem)
    }
    #[inline]
    pub fn neg(self, x: f64) -> f64 {
        fp1!(self, x, neg)
    }
    /// `OpFMod` / GLSL `mod`: `x - y * floor(x / y)`, each step rounded.
    #[inline]
    pub fn fmod(self, x: f64, y: f64) -> f64 {
        let q = self.floor(self.div(x, y));
        self.sub(x, self.mul(y, q))
    }
    #[inline]
    pub fn fma(self, a: f64, b: f64, c: f64) -> f64 {
        match self.prec {
            Prec::P64 => a.mul_add(b, c),
            Prec::P32 => (a as f32).mul_add(b as f32, c as f32) as f64,
            Prec::P16 => r16((a as f32).mul_add(b as f32, c as f32)),
        }
    }
    #[inline]
    pub fn floor(self, x: f64) -> f64 {
        fp1!(self, x, floor)
    }
    #[inline]
    pub fn ceil(self, x: f64) -> f64 {
        fp1!(self, x, ceil)
    }
    #[inline]
    pub fn trunc(self, x: f64) -> f64 {
        fp1!(self, x, trunc)
    }
    #[inline]
    pub fn round_away(self, x: f64) -> f64 {
        fp1!(self, x, round)
    }
    #[inline]
    pub fn round_even(self, x: f64) -> f64 {
        fp1!(self, x, round_ties_even)
    }
    #[inline]
    pub fn abs(self, x: f64) -> f64 {
        fp1!(self, x, abs)
    }
    #[inline]
    pub fn sqrt(self, x: f64) -> f64 {
        fp1!(self, x, sqrt)
    }
    #[inline]
    pub fn sin(self, x: f64) -> f64 {
        fp1!(self, x, sin)
    }
    #[inline]
    pub fn cos(self, x: f64) -> f64 {
        fp1!(self, x, cos)
    }
    #[inline]
    pub fn tan(self, x: f64) -> f64 {
        fp1!(self, x, tan)
    }
    #[inline]
    pub fn asin(self, x: f64) -> f64 {
        fp1!(self, x, asin)
    }
    #[inline]
    pub fn acos(self, x: f64) -> f64 {
        fp1!(self, x, acos)
    }
    #[inline]
    pub fn atan(self, x: f64) -> f64 {
        fp1!(self, x, atan)
    }
    #[inline]
    pub fn sinh(self, x: f64) -> f64 {
        fp1!(self, x, sinh)
    }
    #[inline]
    pub fn cosh(self, x: f64) -> f64 {
        fp1!(self, x, cosh)
    }
    #[inline]
    pub fn tanh(self, x: f64) -> f64 {
        fp1!(self, x, tanh)
    }
    #[inline]
    pub fn asinh(self, x: f64) -> f64 {
        fp1!(self, x, asinh)
    }
    #[inline]
    pub fn acosh(self, x: f64) -> f64 {
        fp1!(self, x, acosh)
    }
    #[inline]
    pub fn atanh(self, x: f64) -> f64 {
        fp1!(self, x, atanh)
    }
    #[inline]
    pub fn atan2(self, y: f64, x: f64) -> f64 {
        fp2!(self, y, x, atan2)
    }
    #[inline]
    pub fn pow(self, x: f64, y: f64) -> f64 {
        fp2!(self, x, y, powf)
    }
    #[inline]
    pub fn exp(self, x: f64) -> f64 {
        fp1!(self, x, exp)
    }
    #[inline]
    pub fn log(self, x: f64) -> f64 {
        fp1!(self, x, ln)
    }
    #[inline]
    pub fn exp2(self, x: f64) -> f64 {
        fp1!(self, x, exp2)
    }
    #[inline]
    pub fn log2(self, x: f64) -> f64 {
        fp1!(self, x, log2)
    }
    /// GLSL `inversesqrt`: `1 / sqrt(x)`, two roundings.
    #[inline]
    pub fn inversesqrt(self, x: f64) -> f64 {
        self.div(1.0, self.sqrt(x))
    }
    #[inline]
    pub fn min(self, x: f64, y: f64) -> f64 {
        // GLSL min: undefined for NaN; use IEEE minNum like most hardware.
        fp2!(self, x, y, min)
    }
    #[inline]
    pub fn max(self, x: f64, y: f64) -> f64 {
        fp2!(self, x, y, max)
    }
    #[inline]
    pub fn clamp(self, x: f64, lo: f64, hi: f64) -> f64 {
        self.min(self.max(x, lo), hi)
    }
    /// `mix(x, y, a) = x * (1 - a) + y * a`, each step rounded (the GLSL definition; hardware
    /// may fuse it).
    #[inline]
    pub fn mix(self, x: f64, y: f64, a: f64) -> f64 {
        self.add(self.mul(x, self.sub(1.0, a)), self.mul(y, a))
    }
    #[inline]
    pub fn step(self, edge: f64, x: f64) -> f64 {
        if x < edge {
            0.0
        } else {
            1.0
        }
    }
    #[inline]
    pub fn smoothstep(self, e0: f64, e1: f64, x: f64) -> f64 {
        let t = self.clamp(self.div(self.sub(x, e0), self.sub(e1, e0)), 0.0, 1.0);
        self.mul(self.mul(t, t), self.sub(3.0, self.mul(2.0, t)))
    }
    #[inline]
    pub fn fract(self, x: f64) -> f64 {
        self.sub(x, self.floor(x))
    }
    #[inline]
    pub fn sign(self, x: f64) -> f64 {
        if x > 0.0 {
            1.0
        } else if x < 0.0 {
            -1.0
        } else {
            0.0
        }
    }
    #[inline]
    pub fn radians(self, x: f64) -> f64 {
        self.mul(x, self.round(std::f64::consts::PI / 180.0))
    }
    #[inline]
    pub fn degrees(self, x: f64) -> f64 {
        self.mul(x, self.round(180.0 / std::f64::consts::PI))
    }
    #[inline]
    pub fn ldexp(self, x: f64, e: i32) -> f64 {
        match self.prec {
            Prec::P64 => x * 2f64.powi(e),
            Prec::P32 => ldexp32(x as f32, e) as f64,
            Prec::P16 => r16(ldexp32(x as f32, e)),
        }
    }
    /// Dot product: products then a left-to-right sum, each step rounded.
    pub fn dot(self, a: &[f64], b: &[f64]) -> f64 {
        let mut acc = 0.0;
        for (i, (x, y)) in a.iter().zip(b).enumerate() {
            let p = self.mul(*x, *y);
            acc = if i == 0 { p } else { self.add(acc, p) };
        }
        acc
    }
}

fn ldexp32(x: f32, e: i32) -> f32 {
    // Two-step scaling avoids overflow of the intermediate 2^e.
    let e = e.clamp(-300, 300);
    let h = e / 2;
    x * 2f32.powi(h) * 2f32.powi(e - h)
}

/// `frexp`: returns (mantissa in [0.5, 1), exponent) with `x = m * 2^e`; zero/inf/nan give (x, 0).
pub fn frexp(x: f64) -> (f64, i32) {
    if x == 0.0 || !x.is_finite() {
        return (x, 0);
    }
    let bits = x.to_bits();
    let exp = ((bits >> 52) & 0x7ff) as i32;
    if exp == 0 {
        // Subnormal: scale up first.
        let (m, e) = frexp(x * 2f64.powi(64));
        return (m, e - 64);
    }
    let e = exp - 1022;
    let m = f64::from_bits((bits & !(0x7ffu64 << 52)) | (1022u64 << 52));
    (m, e)
}

/// Helper: wrapping 32-bit integer arithmetic on either signedness; the result signedness is
/// given by the caller (from the result type).
#[inline]
pub fn mk_int(signed: bool, bits: u32) -> Value {
    if signed {
        Value::I32(bits as i32)
    } else {
        Value::U32(bits)
    }
}

/// Float-to-half-float-to-float round trip used by `QuantizeToF16` and `Pack/UnpackHalf2x16`.
pub fn to_f16_bits(x: f64) -> u16 {
    half::f16::from_f32(x as f32).to_bits()
}

pub fn from_f16_bits(b: u16) -> f64 {
    half::f16::from_bits(b).to_f64()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frexp_matches() {
        for &x in &[1.0, 0.75, 8.0, -3.0, 1e-310, 123456.789] {
            let (m, e) = frexp(x);
            assert_eq!(m * 2f64.powi(e), x);
            assert!(m.abs() >= 0.5 && m.abs() < 1.0, "{x}: {m} {e}");
        }
    }

    #[test]
    fn f16_rounding() {
        let fp = Fp { prec: Prec::P16 };
        assert_eq!(fp.add(1.0, 2f64.powi(-12)), 1.0);
        assert_eq!(fp.add(1.0, 2f64.powi(-10)), 1.0 + 2f64.powi(-10));
        let fp32 = Fp { prec: Prec::P32 };
        assert_eq!(fp32.add(1.0, 2f64.powi(-12)), 1.0 + 2f64.powi(-12));
    }
}
