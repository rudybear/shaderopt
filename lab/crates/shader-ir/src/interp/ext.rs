//! `GLSL.std.450` extended instructions.
//!
//! Scalar float functions use the numeric mode's precision (f32 `libm` results in `f32`/`f16`
//! modes, f64 in `f64` mode). Compound functions follow the GLSL definitions step by step with a
//! rounding after every step (`mix`, `smoothstep`, `normalize = v * inversesqrt(dot(v, v))`,
//! `length = sqrt(dot(v, v))`, `reflect`, `refract`); GPUs may fuse steps, so these are
//! approximations at the ULP level. `Determinant` and `MatrixInverse` are computed in f64 and
//! rounded once.

use super::exec::Invocation;
use super::value::{frexp, from_f16_bits, map1, map2, map3, mk_int, to_f16_bits, Fp, Value};
use super::Kind;
use anyhow::{anyhow, bail, Result};
use rspirv::dr::{Instruction, Operand};
use spirv::GLOp;

fn vec_f(fp: Fp, v: &[f64]) -> Value {
    Value::V(v.iter().map(|x| Value::F(fp.round(*x))).collect())
}

fn sub_v(fp: Fp, a: &[f64], b: &[f64]) -> Vec<f64> {
    a.iter().zip(b).map(|(x, y)| fp.sub(*x, *y)).collect()
}

fn scale_v(fp: Fp, a: &[f64], s: f64) -> Vec<f64> {
    a.iter().map(|x| fp.mul(*x, s)).collect()
}

/// Evaluates GLSL.std.450 instruction `n` of `inst`.
pub fn glsl(inv: &mut Invocation<'_>, inst: &Instruction, n: u32) -> Result<Value> {
    let op = GLOp::from_u32(n).ok_or_else(|| anyhow!("unknown GLSL.std.450 instruction #{n}"))?;
    let rt = inst.result_type.ok_or_else(|| anyhow!("OpExtInst without result type"))?;
    let ids: Vec<u32> = inst.operands[2..]
        .iter()
        .map(|o| match o {
            Operand::IdRef(id) => Ok(*id),
            other => Err(anyhow!("unexpected ext-inst operand {other:?}")),
        })
        .collect::<Result<_>>()?;
    let args: Vec<Value> = ids.iter().map(|id| inv.vals[*id as usize].clone()).collect();
    let arg = |i: usize| -> Result<&Value> { args.get(i).ok_or_else(|| anyhow!("{op:?}: missing operand {i}")) };
    let kind = inv.prog.scalar_kind(rt);
    let fp = match kind {
        Ok(Kind::Float { width }) => Fp { prec: inv.prog.mode.prec(width) },
        _ => Fp { prec: super::value::Prec::P64 },
    };
    let signed = matches!(kind, Ok(Kind::Int { signed: true }));

    macro_rules! f1 {
        ($m:ident) => {
            map1(arg(0)?, &mut |x| Ok(Value::F(fp.$m(x.as_f()?))))
        };
    }
    macro_rules! f2 {
        ($m:ident) => {
            map2(arg(0)?, arg(1)?, &mut |x, y| Ok(Value::F(fp.$m(x.as_f()?, y.as_f()?))))
        };
    }
    macro_rules! f3 {
        ($m:ident) => {
            map3(arg(0)?, arg(1)?, arg(2)?, &mut |x, y, z| Ok(Value::F(fp.$m(x.as_f()?, y.as_f()?, z.as_f()?))))
        };
    }
    macro_rules! i1 {
        ($f:expr) => {
            map1(arg(0)?, &mut |x| Ok(mk_int(signed, $f(x.as_bits()?))))
        };
    }
    macro_rules! i2 {
        ($f:expr) => {
            map2(arg(0)?, arg(1)?, &mut |x, y| Ok(mk_int(signed, $f(x.as_bits()?, y.as_bits()?))))
        };
    }
    macro_rules! i3 {
        ($f:expr) => {
            map3(arg(0)?, arg(1)?, arg(2)?, &mut |x, y, z| Ok(mk_int(signed, $f(x.as_bits()?, y.as_bits()?, z.as_bits()?))))
        };
    }

    match op {
        GLOp::Round => f1!(round_away),
        GLOp::RoundEven => f1!(round_even),
        GLOp::Trunc => f1!(trunc),
        GLOp::FAbs => f1!(abs),
        GLOp::SAbs => i1!(|a: u32| (a as i32).wrapping_abs() as u32),
        GLOp::FSign => f1!(sign),
        GLOp::SSign => i1!(|a: u32| (a as i32).signum() as u32),
        GLOp::Floor => f1!(floor),
        GLOp::Ceil => f1!(ceil),
        GLOp::Fract => f1!(fract),
        GLOp::Radians => f1!(radians),
        GLOp::Degrees => f1!(degrees),
        GLOp::Sin => f1!(sin),
        GLOp::Cos => f1!(cos),
        GLOp::Tan => f1!(tan),
        GLOp::Asin => f1!(asin),
        GLOp::Acos => f1!(acos),
        GLOp::Atan => f1!(atan),
        GLOp::Sinh => f1!(sinh),
        GLOp::Cosh => f1!(cosh),
        GLOp::Tanh => f1!(tanh),
        GLOp::Asinh => f1!(asinh),
        GLOp::Acosh => f1!(acosh),
        GLOp::Atanh => f1!(atanh),
        GLOp::Atan2 => f2!(atan2),
        GLOp::Pow => f2!(pow),
        GLOp::Exp => f1!(exp),
        GLOp::Log => f1!(log),
        GLOp::Exp2 => f1!(exp2),
        GLOp::Log2 => f1!(log2),
        GLOp::Sqrt => f1!(sqrt),
        GLOp::InverseSqrt => f1!(inversesqrt),
        GLOp::Determinant => {
            let m = matrix(arg(0)?)?;
            Ok(Value::F(fp.round(det(&m)?)))
        }
        GLOp::MatrixInverse => {
            let m = matrix(arg(0)?)?;
            let inv_m = inverse(&m)?;
            Ok(Value::V(inv_m.iter().map(|c| vec_f(fp, c)).collect()))
        }
        GLOp::Modf | GLOp::ModfStruct => {
            let x = arg(0)?;
            let whole = map1(x, &mut |v| Ok(Value::F(fp.trunc(v.as_f()?))))?;
            let frac = map2(x, &whole, &mut |v, w| Ok(Value::F(fp.sub(v.as_f()?, w.as_f()?))))?;
            if op == GLOp::Modf {
                let p = match arg(1)? {
                    Value::Ptr(p) => p.clone(),
                    other => bail!("Modf: second operand is {}, not a pointer", other.describe()),
                };
                inv.write_ptr(&p, whole)?;
                Ok(frac)
            } else {
                Ok(Value::V(vec![frac, whole]))
            }
        }
        GLOp::FMin | GLOp::NMin => f2!(min),
        GLOp::FMax | GLOp::NMax => f2!(max),
        GLOp::UMin => i2!(|a: u32, b: u32| a.min(b)),
        GLOp::SMin => i2!(|a: u32, b: u32| (a as i32).min(b as i32) as u32),
        GLOp::UMax => i2!(|a: u32, b: u32| a.max(b)),
        GLOp::SMax => i2!(|a: u32, b: u32| (a as i32).max(b as i32) as u32),
        GLOp::FClamp | GLOp::NClamp => f3!(clamp),
        GLOp::UClamp => i3!(|x: u32, lo: u32, hi: u32| x.max(lo).min(hi)),
        GLOp::SClamp => i3!(|x: u32, lo: u32, hi: u32| (x as i32).max(lo as i32).min(hi as i32) as u32),
        GLOp::FMix => f3!(mix),
        GLOp::IMix => bail!("IMix is not supported"),
        GLOp::Step => f2!(step),
        GLOp::SmoothStep => f3!(smoothstep),
        GLOp::Fma => f3!(fma),
        GLOp::Frexp | GLOp::FrexpStruct => {
            let x = arg(0)?;
            let mant = map1(x, &mut |v| Ok(Value::F(fp.round(frexp(v.as_f()?).0))))?;
            let exp = map1(x, &mut |v| Ok(Value::I32(frexp(v.as_f()?).1)))?;
            if op == GLOp::Frexp {
                let p = match arg(1)? {
                    Value::Ptr(p) => p.clone(),
                    other => bail!("Frexp: second operand is {}, not a pointer", other.describe()),
                };
                inv.write_ptr(&p, exp)?;
                Ok(mant)
            } else {
                Ok(Value::V(vec![mant, exp]))
            }
        }
        GLOp::Ldexp => {
            let x = arg(0)?;
            let e = arg(1)?;
            match e {
                Value::V(_) => map2(x, e, &mut |v, e| Ok(Value::F(fp.ldexp(v.as_f()?, e.as_bits()? as i32)))),
                e => {
                    let e = e.as_bits()? as i32;
                    map1(x, &mut |v| Ok(Value::F(fp.ldexp(v.as_f()?, e))))
                }
            }
        }
        GLOp::PackHalf2x16 => {
            let v = arg(0)?.floats()?;
            if v.len() != 2 {
                bail!("PackHalf2x16 needs a vec2");
            }
            Ok(Value::U32(to_f16_bits(v[0]) as u32 | ((to_f16_bits(v[1]) as u32) << 16)))
        }
        GLOp::UnpackHalf2x16 => {
            let b = arg(0)?.as_bits()?;
            Ok(Value::V(vec![Value::F(fp.round(from_f16_bits(b as u16))), Value::F(fp.round(from_f16_bits((b >> 16) as u16)))]))
        }
        GLOp::PackUnorm4x8 | GLOp::PackSnorm4x8 => {
            let v = arg(0)?.floats()?;
            if v.len() != 4 {
                bail!("{op:?} needs a vec4");
            }
            let snorm = op == GLOp::PackSnorm4x8;
            let mut out = 0u32;
            for (i, c) in v.iter().enumerate() {
                let q = if snorm { (c.clamp(-1.0, 1.0) * 127.0).round() as i32 as u8 } else { (c.clamp(0.0, 1.0) * 255.0).round() as u8 };
                out |= (q as u32) << (8 * i);
            }
            Ok(Value::U32(out))
        }
        GLOp::PackUnorm2x16 | GLOp::PackSnorm2x16 => {
            let v = arg(0)?.floats()?;
            if v.len() != 2 {
                bail!("{op:?} needs a vec2");
            }
            let snorm = op == GLOp::PackSnorm2x16;
            let mut out = 0u32;
            for (i, c) in v.iter().enumerate() {
                let q = if snorm { (c.clamp(-1.0, 1.0) * 32767.0).round() as i32 as u16 } else { (c.clamp(0.0, 1.0) * 65535.0).round() as u16 };
                out |= (q as u32) << (16 * i);
            }
            Ok(Value::U32(out))
        }
        GLOp::UnpackUnorm4x8 => {
            let b = arg(0)?.as_bits()?;
            Ok(Value::V((0..4).map(|i| Value::F(fp.round(((b >> (8 * i)) & 0xff) as f64 / 255.0))).collect()))
        }
        GLOp::UnpackSnorm4x8 => {
            let b = arg(0)?.as_bits()?;
            Ok(Value::V((0..4).map(|i| Value::F(fp.round((((b >> (8 * i)) & 0xff) as u8 as i8 as f64 / 127.0).clamp(-1.0, 1.0)))).collect()))
        }
        GLOp::UnpackUnorm2x16 => {
            let b = arg(0)?.as_bits()?;
            Ok(Value::V((0..2).map(|i| Value::F(fp.round(((b >> (16 * i)) & 0xffff) as f64 / 65535.0))).collect()))
        }
        GLOp::UnpackSnorm2x16 => {
            let b = arg(0)?.as_bits()?;
            Ok(Value::V((0..2).map(|i| Value::F(fp.round((((b >> (16 * i)) & 0xffff) as u16 as i16 as f64 / 32767.0).clamp(-1.0, 1.0)))).collect()))
        }
        GLOp::PackDouble2x32 | GLOp::UnpackDouble2x32 => bail!("{op:?} is not supported"),
        GLOp::Length => {
            let v = arg(0)?.floats()?;
            Ok(Value::F(fp.sqrt(fp.dot(&v, &v))))
        }
        GLOp::Distance => {
            let d = sub_v(fp, &arg(0)?.floats()?, &arg(1)?.floats()?);
            Ok(Value::F(fp.sqrt(fp.dot(&d, &d))))
        }
        GLOp::Cross => {
            let a = arg(0)?.floats()?;
            let b = arg(1)?.floats()?;
            if a.len() != 3 || b.len() != 3 {
                bail!("Cross needs vec3 operands");
            }
            let c = |i: usize, j: usize| fp.sub(fp.mul(a[i], b[j]), fp.mul(a[j], b[i]));
            Ok(Value::V(vec![Value::F(c(1, 2)), Value::F(c(2, 0)), Value::F(c(0, 1))]))
        }
        GLOp::Normalize => {
            let v = arg(0)?.floats()?;
            let s = fp.inversesqrt(fp.dot(&v, &v));
            Ok(vec_f(fp, &scale_v(fp, &v, s)))
        }
        GLOp::FaceForward => {
            let n = arg(0)?.floats()?;
            let i = arg(1)?.floats()?;
            let nref = arg(2)?.floats()?;
            let flip = fp.dot(&nref, &i) >= 0.0;
            Ok(vec_f(fp, &n.iter().map(|x| if flip { fp.neg(*x) } else { *x }).collect::<Vec<_>>()))
        }
        GLOp::Reflect => {
            let i = arg(0)?.floats()?;
            let n = arg(1)?.floats()?;
            let k = fp.mul(2.0, fp.dot(&n, &i));
            Ok(vec_f(fp, &sub_v(fp, &i, &scale_v(fp, &n, k))))
        }
        GLOp::Refract => {
            let i = arg(0)?.floats()?;
            let n = arg(1)?.floats()?;
            let eta = arg(2)?.as_f()?;
            let ndi = fp.dot(&n, &i);
            let k = fp.sub(1.0, fp.mul(fp.mul(eta, eta), fp.sub(1.0, fp.mul(ndi, ndi))));
            if k < 0.0 {
                Ok(Value::V(vec![Value::F(0.0); i.len()]))
            } else {
                let s = fp.add(fp.mul(eta, ndi), fp.sqrt(k));
                Ok(vec_f(fp, &sub_v(fp, &scale_v(fp, &i, eta), &scale_v(fp, &n, s))))
            }
        }
        GLOp::FindILsb => i1!(|a: u32| if a == 0 { u32::MAX } else { a.trailing_zeros() }),
        GLOp::FindSMsb => i1!(|a: u32| {
            let s = a as i32;
            let v = if s < 0 { !a } else { a };
            if v == 0 { u32::MAX } else { 31 - v.leading_zeros() }
        }),
        GLOp::FindUMsb => i1!(|a: u32| if a == 0 { u32::MAX } else { 31 - a.leading_zeros() }),
        GLOp::InterpolateAtCentroid | GLOp::InterpolateAtSample | GLOp::InterpolateAtOffset => {
            bail!("{op:?} is not supported")
        }
    }
}

/// Column-major matrix as `cols[j][i]`.
fn matrix(v: &Value) -> Result<Vec<Vec<f64>>> {
    v.as_vec()?.iter().map(|c| c.floats()).collect()
}

fn det(m: &[Vec<f64>]) -> Result<f64> {
    let n = m.len();
    if n == 0 || m.iter().any(|c| c.len() != n) {
        bail!("Determinant of a non-square matrix");
    }
    // Gaussian elimination with partial pivoting on a row-major copy.
    let mut a: Vec<Vec<f64>> = (0..n).map(|i| (0..n).map(|j| m[j][i]).collect()).collect();
    let mut d = 1.0;
    for k in 0..n {
        let p = (k..n).max_by(|&x, &y| a[x][k].abs().partial_cmp(&a[y][k].abs()).unwrap_or(std::cmp::Ordering::Equal)).unwrap();
        if a[p][k] == 0.0 {
            return Ok(0.0);
        }
        if p != k {
            a.swap(p, k);
            d = -d;
        }
        d *= a[k][k];
        for i in k + 1..n {
            let f = a[i][k] / a[k][k];
            for j in k..n {
                a[i][j] -= f * a[k][j];
            }
        }
    }
    Ok(d)
}

fn inverse(m: &[Vec<f64>]) -> Result<Vec<Vec<f64>>> {
    let n = m.len();
    if n == 0 || m.iter().any(|c| c.len() != n) {
        bail!("MatrixInverse of a non-square matrix");
    }
    let mut a: Vec<Vec<f64>> = (0..n).map(|i| (0..n).map(|j| m[j][i]).collect()).collect();
    let mut inv: Vec<Vec<f64>> = (0..n).map(|i| (0..n).map(|j| if i == j { 1.0 } else { 0.0 }).collect()).collect();
    for k in 0..n {
        let p = (k..n).max_by(|&x, &y| a[x][k].abs().partial_cmp(&a[y][k].abs()).unwrap_or(std::cmp::Ordering::Equal)).unwrap();
        a.swap(p, k);
        inv.swap(p, k);
        let piv = a[k][k];
        // Singular: GLSL says the result is undefined; produce inf/nan like a GPU would.
        for j in 0..n {
            a[k][j] /= piv;
            inv[k][j] /= piv;
        }
        for i in 0..n {
            if i != k {
                let f = a[i][k];
                for j in 0..n {
                    a[i][j] -= f * a[k][j];
                    inv[i][j] -= f * inv[k][j];
                }
            }
        }
    }
    // Back to column-major.
    Ok((0..n).map(|j| (0..n).map(|i| inv[i][j]).collect()).collect())
}
