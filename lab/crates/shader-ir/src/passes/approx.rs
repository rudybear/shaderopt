//! `approx`: range-profiled polynomial approximation of `GLSL.std.450` transcendentals (M4,
//! `shader-ir approx`). Class `lossy`.
//!
//! For every listed site (`Exp`, `Exp2`, `Log`, `Log2`, `Pow` with a constant exponent in
//! `(0, 4]`, `Sin`, `Cos`, `Sqrt`, `InverseSqrt`; f32 scalar or vector) the observed
//! `[min, max]` of the site's *operand* (its id in `ranges.json` from `eval --profile`) is
//! padded by 5% of its width and a polynomial in `t = x * A + B` (`t` in `[-1, 1]` over the
//! range) is fitted: weighted least squares in the Chebyshev basis at 512 Chebyshev nodes with
//! weights `1 / |f|` (so the fit minimizes *relative* error), converted to monomials in `t`,
//! coefficients rounded to f32. The degree is the smallest in the requested range whose maximum
//! relative error over 4096 evenly spaced points, evaluated exactly as the shader will (f32
//! `Fma` Horner) against the f64 function, is `<= max_rel_err`; otherwise the site is skipped
//! with the best error as the reason. The relative error is `|p - f| / max(|f|, floor)` with
//! `floor = 0` for `Exp`/`Exp2`/`Sqrt`/`InverseSqrt`/`Pow` and `floor = 0.01 * max |f|` over
//! the range for `Sin`/`Cos`/`Log`/`Log2`, which have zeros. `Pow(x, c)` is approximated
//! directly as a polynomial in `x` (not through `exp2(c * log2(x))`).
//!
//! The site keeps its result id: `t = Fma(x, A, B)`, `acc = c_n`, `acc = Fma(acc, t, c_k)` down
//! to `c_0`, the last `Fma` carrying the original id. Vector sites get the same scalar
//! polynomial componentwise through splat constants. **No range guard is emitted**: the range
//! comes from the profiled scenarios; inputs outside it get the polynomial's extrapolation, and
//! the lab's holdout scenarios are the check.

use super::consts;
use super::{float_width, fresh_id, glsl_op, id_op, vector_shape, Class, EditOp};
use crate::analysis::Range;
use crate::lift::Lifted;
use anyhow::{anyhow, bail, Result};
use rspirv::dr::{Instruction, Operand};
use spirv::{GLOp, Op};
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct ApproxOpts {
    pub sites: Vec<u32>,
    /// Inclusive degree range tried in ascending order.
    pub degree: (u32, u32),
    pub max_rel_err: f64,
}

impl Default for ApproxOpts {
    fn default() -> Self {
        ApproxOpts { sites: Vec::new(), degree: (3, 7), max_rel_err: 1e-3 }
    }
}

/// Parses `3..7` or `5`.
pub fn parse_degree(s: &str) -> Result<(u32, u32)> {
    let (lo, hi) = match s.split_once("..") {
        Some((a, b)) => (a.trim().parse::<u32>()?, b.trim().parse::<u32>()?),
        None => {
            let d = s.trim().parse::<u32>()?;
            (d, d)
        }
    };
    if lo < 1 || hi < lo || hi > 16 {
        bail!("--degree {s:?}: expected LO..HI with 1 <= LO <= HI <= 16");
    }
    Ok((lo, hi))
}

/// What happened at one site.
#[derive(Clone, Debug)]
pub struct SiteReport {
    pub id: u32,
    pub op: String,
    pub ty: String,
    /// Fitted (padded) range, when the operand had one.
    pub range: Option<(f64, f64)>,
    pub degree: Option<u32>,
    pub max_rel_err: Option<f64>,
    /// `replaced`, or the reason the site was skipped.
    pub status: String,
}

impl SiteReport {
    pub fn replaced(&self) -> bool {
        self.status == "replaced"
    }
}

/// A fitted polynomial in `t = x * a + b`: `p(t) = sum c[k] t^k` with f32 coefficients.
#[derive(Clone, Debug)]
pub struct Fit {
    pub degree: u32,
    pub a: f32,
    pub b: f32,
    pub coeffs: Vec<f32>,
    pub max_rel_err: f64,
}

const NODES: usize = 512;
const TEST_POINTS: usize = 4096;
const PADDING: f64 = 0.05;

/// Runs the pass on the listed sites; returns one report per site (listed order).
pub fn run(l: &mut Lifted, ranges: &BTreeMap<u32, Range>, opts: &ApproxOpts, ops: &mut Vec<EditOp>) -> Result<Vec<SiteReport>> {
    let mut reports = Vec::new();
    // (site index into module, fit, report index)
    let mut edits: Vec<((usize, usize, usize), Fit, GLOp)> = Vec::new();
    for &id in &opts.sites {
        let (site, inst) = match find_site(l, id) {
            Some(x) => x,
            None => {
                reports.push(SiteReport { id, op: "?".into(), ty: "?".into(), range: None, degree: None, max_rel_err: None, status: "not a GLSL.std.450 instruction in a function body".into() });
                continue;
            }
        };
        let inst = inst.clone();
        let g = glsl_op(l, &inst).ok_or_else(|| anyhow!("%{id} is not a GLSL.std.450 instruction"))?;
        let ty = inst.result_type.unwrap();
        let mut rep = SiteReport { id, op: format!("{g:?}"), ty: l.type_name(ty), range: None, degree: None, max_rel_err: None, status: String::new() };
        let skip = |rep: &mut SiteReport, why: String| rep.status = why;
        if !matches!(g, GLOp::Exp | GLOp::Exp2 | GLOp::Log | GLOp::Log2 | GLOp::Pow | GLOp::Sin | GLOp::Cos | GLOp::Sqrt | GLOp::InverseSqrt) {
            skip(&mut rep, format!("{g:?} is not an approximable op"));
            reports.push(rep);
            continue;
        }
        if float_width(l, ty) != Some(32) || vector_shape(l, ty).is_none() {
            skip(&mut rep, format!("result type {} is not an f32 scalar/vector", l.type_name(ty)));
            reports.push(rep);
            continue;
        }
        let Some(x) = id_op(&inst, 2) else {
            skip(&mut rep, "missing operand".into());
            reports.push(rep);
            continue;
        };
        // Pow: constant scalar/splat exponent in (0, 4].
        let mut exponent = 1.0;
        if g == GLOp::Pow {
            let e = id_op(&inst, 3).and_then(|e| consts::read(l, e));
            let c = e.as_ref().and_then(|cv| {
                let comps = cv.comps();
                let first = comps.first()?.as_f64()?;
                comps.iter().all(|c| c.as_f64() == Some(first)).then_some(first)
            });
            match c {
                Some(c) if c > 0.0 && c <= 4.0 => exponent = c,
                Some(c) => {
                    skip(&mut rep, format!("Pow exponent {c} is outside (0, 4]"));
                    reports.push(rep);
                    continue;
                }
                None => {
                    skip(&mut rep, "Pow exponent is not a constant scalar/splat".into());
                    reports.push(rep);
                    continue;
                }
            }
            rep.op = format!("Pow(x, {})", fmt_g(exponent));
        }
        let Some(r) = ranges.get(&x) else {
            skip(&mut rep, format!("operand %{x} has no range in ranges.json"));
            reports.push(rep);
            continue;
        };
        if r.samples == 0 || !r.min.is_finite() || !r.max.is_finite() {
            skip(&mut rep, format!("operand %{x} has no finite samples"));
            reports.push(rep);
            continue;
        }
        if r.nan > 0 || r.inf > 0 {
            skip(&mut rep, format!("operand %{x} had {} NaN and {} inf samples", r.nan, r.inf));
            reports.push(rep);
            continue;
        }
        let (lo, hi) = match padded_range(g, r.min, r.max) {
            Ok(x) => x,
            Err(why) => {
                skip(&mut rep, why);
                reports.push(rep);
                continue;
            }
        };
        rep.range = Some((lo, hi));
        let f: Box<dyn Fn(f64) -> f64> = match g {
            GLOp::Exp => Box::new(|x| x.exp()),
            GLOp::Exp2 => Box::new(|x| x.exp2()),
            GLOp::Log => Box::new(|x| x.ln()),
            GLOp::Log2 => Box::new(|x| x.log2()),
            GLOp::Sin => Box::new(|x| x.sin()),
            GLOp::Cos => Box::new(|x| x.cos()),
            GLOp::Sqrt => Box::new(|x| x.sqrt()),
            GLOp::InverseSqrt => Box::new(|x| 1.0 / x.sqrt()),
            GLOp::Pow => Box::new(move |x| x.powf(exponent)),
            _ => unreachable!(),
        };
        let floor = if matches!(g, GLOp::Sin | GLOp::Cos | GLOp::Log | GLOp::Log2) {
            0.01 * (0..TEST_POINTS).map(|i| f(lo + (hi - lo) * i as f64 / (TEST_POINTS - 1) as f64).abs()).fold(0.0, f64::max)
        } else {
            0.0
        };
        let mut best: Option<Fit> = None;
        let mut chosen: Option<Fit> = None;
        for n in opts.degree.0..=opts.degree.1 {
            let Some(fit) = fit_poly(&*f, lo, hi, n as usize, floor) else { continue };
            if fit.max_rel_err <= opts.max_rel_err {
                chosen = Some(fit);
                break;
            }
            if best.as_ref().map_or(true, |b| fit.max_rel_err < b.max_rel_err) {
                best = Some(fit);
            }
        }
        match chosen {
            Some(fit) => {
                rep.degree = Some(fit.degree);
                rep.max_rel_err = Some(fit.max_rel_err);
                rep.status = "replaced".into();
                edits.push((site, fit, g));
            }
            None => {
                let why = match &best {
                    Some(b) => format!(
                        "no degree in {}..{} reaches max rel err {:.1e} on [{}, {}]: best degree {} has {:.1e}",
                        opts.degree.0,
                        opts.degree.1,
                        opts.max_rel_err,
                        fmt_g(lo),
                        fmt_g(hi),
                        b.degree,
                        b.max_rel_err
                    ),
                    None => "the function is not finite on the range".into(),
                };
                if let Some(b) = best {
                    rep.degree = Some(b.degree);
                    rep.max_rel_err = Some(b.max_rel_err);
                }
                rep.status = why;
            }
        }
        reports.push(rep);
    }
    if edits.is_empty() {
        return Ok(reports);
    }
    // Emit in descending layout order so that insertions never shift a pending site.
    edits.sort_by_key(|(s, _, _)| std::cmp::Reverse(*s));
    for ((fi, bi, ii), fit, g) in edits {
        let inst = l.module.functions[fi].blocks[bi].instructions[ii].clone();
        let id = inst.result_id.unwrap();
        let ty = inst.result_type.unwrap();
        let set = id_op(&inst, 0).unwrap();
        let x = id_op(&inst, 2).unwrap();
        let c = |l: &mut Lifted, v: f32| consts::splat_f(l, ty, v as f64).expect("f32 splat constant");
        let a = c(l, fit.a);
        let b = c(l, fit.b);
        let coeffs: Vec<u32> = fit.coeffs.iter().map(|&v| c(l, v)).collect();
        let fma = |r: u32, p: u32, q: u32, s: u32| {
            Instruction::new(
                Op::ExtInst,
                Some(ty),
                Some(r),
                vec![Operand::IdRef(set), Operand::LiteralExtInstInteger(GLOp::Fma as u32), Operand::IdRef(p), Operand::IdRef(q), Operand::IdRef(s)],
            )
        };
        let n = fit.coeffs.len() - 1;
        let t = fresh_id(l);
        let mut new_insts = vec![fma(t, x, a, b)];
        let mut acc = coeffs[n];
        for k in (0..n).rev() {
            let r = if k == 0 { id } else { fresh_id(l) };
            new_insts.push(fma(r, acc, t, coeffs[k]));
            acc = r;
        }
        let insts = &mut l.module.functions[fi].blocks[bi].instructions;
        insts.remove(ii);
        for (k, ni) in new_insts.into_iter().enumerate() {
            insts.insert(ii + k, ni);
        }
        let (lo, hi) = fit_range(&fit);
        let what = if g == GLOp::Pow { reports.iter().find(|r| r.id == id).map(|r| r.op.clone()).unwrap_or("Pow".into()) } else { format!("{g:?}") };
        ops.push(EditOp {
            pass: "approx",
            class: Class::Lossy,
            target: id,
            replaced_by: Some(id),
            detail: format!("{what} -> degree-{} polynomial on [{}, {}], max rel err {:.1e}", fit.degree, fmt_g(lo), fmt_g(hi), fit.max_rel_err),
        });
    }
    l.reanalyze()?;
    Ok(reports)
}

/// The fitted `[lo, hi]` back from `a`, `b` (`t = x a + b`).
fn fit_range(fit: &Fit) -> (f64, f64) {
    let (a, b) = (fit.a as f64, fit.b as f64);
    ((-1.0 - b) / a, (1.0 - b) / a)
}

fn find_site(l: &Lifted, id: u32) -> Option<((usize, usize, usize), &Instruction)> {
    match l.defs.get(&id)? {
        crate::lift::Site::Inst(fi, bi, ii) => {
            let inst = &l.module.functions[*fi].blocks[*bi].instructions[*ii];
            (inst.class.opcode == Op::ExtInst).then_some(((*fi, *bi, *ii), inst))
        }
        _ => None,
    }
}

/// 5% padding, then the domain rules of the function.
fn padded_range(g: GLOp, min: f64, max: f64) -> std::result::Result<(f64, f64), String> {
    let width = max - min;
    let pad = if width > 0.0 { PADDING * width } else { (PADDING * min.abs()).max(1e-6) };
    let mut lo = min - pad;
    let hi = max + pad;
    match g {
        GLOp::Log | GLOp::Log2 | GLOp::InverseSqrt => {
            if min <= 0.0 {
                return Err(format!("{g:?}: operand range [{}, {}] includes non-positive values", fmt_g(min), fmt_g(max)));
            }
            lo = lo.max(min * (1.0 - PADDING));
        }
        GLOp::Sqrt | GLOp::Pow => {
            if min < 0.0 {
                return Err(format!("{g:?}: operand range [{}, {}] includes negative values", fmt_g(min), fmt_g(max)));
            }
            lo = if min > 0.0 { lo.max(min * (1.0 - PADDING)) } else { 0.0 };
        }
        _ => {}
    }
    if !(hi > lo) {
        return Err(format!("degenerate range [{}, {}]", fmt_g(lo), fmt_g(hi)));
    }
    Ok((lo, hi))
}

pub fn fmt_g(x: f64) -> String {
    if x == 0.0 {
        "0.0".into()
    } else if x.abs() >= 1e-3 && x.abs() < 1e6 {
        let s = format!("{x:.4}");
        let s = s.trim_end_matches('0');
        if s.ends_with('.') {
            format!("{s}0")
        } else {
            s.to_string()
        }
    } else {
        format!("{x:.3e}")
    }
}

// ---- fitting ---------------------------------------------------------------------------------

/// Fits a degree-`n` polynomial to `f` on `[lo, hi]` (see the module doc) and measures its
/// f32 Horner error. `None` when the function is not finite at a node.
pub fn fit_poly(f: &dyn Fn(f64) -> f64, lo: f64, hi: f64, n: usize, floor: f64) -> Option<Fit> {
    let m = NODES;
    let k = n + 1;
    let half = (hi - lo) / 2.0;
    let mid = (hi + lo) / 2.0;
    // Weighted design matrix in the Chebyshev basis.
    let mut a: Vec<Vec<f64>> = Vec::with_capacity(m);
    let mut b: Vec<f64> = Vec::with_capacity(m);
    for j in 0..m {
        let t = ((2 * j + 1) as f64 * std::f64::consts::PI / (2 * m) as f64).cos();
        let x = t * half + mid;
        let y = f(x);
        if !y.is_finite() {
            return None;
        }
        let w = 1.0 / y.abs().max(floor).max(1e-300);
        let mut row = Vec::with_capacity(k);
        let (mut t0, mut t1) = (1.0, t);
        for i in 0..k {
            let v = if i == 0 {
                1.0
            } else if i == 1 {
                t
            } else {
                let t2 = 2.0 * t * t1 - t0;
                t0 = t1;
                t1 = t2;
                t2
            };
            row.push(w * v);
        }
        a.push(row);
        b.push(w * y);
    }
    let c = lstsq(a, b)?;
    let mono = cheb_to_mono(&c);
    let a32 = (1.0 / half) as f32;
    let b32 = (-mid / half) as f32;
    let coeffs: Vec<f32> = mono.iter().map(|v| *v as f32).collect();
    let mut fit = Fit { degree: n as u32, a: a32, b: b32, coeffs, max_rel_err: 0.0 };
    fit.max_rel_err = measure(f, lo, hi, &fit, floor);
    Some(fit)
}

/// Maximum relative error of the f32 Horner evaluation over evenly spaced points.
pub fn measure(f: &dyn Fn(f64) -> f64, lo: f64, hi: f64, fit: &Fit, floor: f64) -> f64 {
    let mut worst: f64 = 0.0;
    for i in 0..TEST_POINTS {
        let x = lo + (hi - lo) * i as f64 / (TEST_POINTS - 1) as f64;
        let x32 = x as f32;
        let p = eval_f32(fit, x32) as f64;
        let truth = f(x32 as f64);
        let denom = truth.abs().max(floor);
        let e = if denom > 0.0 { (p - truth).abs() / denom } else if p == truth { 0.0 } else { f64::INFINITY };
        if !e.is_finite() {
            return f64::INFINITY;
        }
        worst = worst.max(e);
    }
    worst
}

/// Exactly what the shader computes: `t = fma(x, a, b)`, Horner with `fma` in f32.
pub fn eval_f32(fit: &Fit, x: f32) -> f32 {
    let t = x.mul_add(fit.a, fit.b);
    let n = fit.coeffs.len() - 1;
    let mut acc = fit.coeffs[n];
    for k in (0..n).rev() {
        acc = acc.mul_add(t, fit.coeffs[k]);
    }
    acc
}

/// Chebyshev series coefficients -> monomial coefficients in `t`.
fn cheb_to_mono(c: &[f64]) -> Vec<f64> {
    let n = c.len();
    let mut p = vec![0.0; n];
    let mut t_prev: Vec<f64> = vec![1.0];
    let mut t_cur: Vec<f64> = vec![0.0, 1.0];
    for (k, ck) in c.iter().enumerate() {
        let tk: Vec<f64> = match k {
            0 => t_prev.clone(),
            1 => t_cur.clone(),
            _ => {
                let mut next = vec![0.0; k + 1];
                for (i, v) in t_cur.iter().enumerate() {
                    next[i + 1] += 2.0 * v;
                }
                for (i, v) in t_prev.iter().enumerate() {
                    next[i] -= v;
                }
                t_prev = std::mem::replace(&mut t_cur, next.clone());
                next
            }
        };
        for (i, v) in tk.iter().enumerate() {
            p[i] += ck * v;
        }
    }
    p
}

/// Least squares `min |A c - b|` by Householder QR (m rows, k columns, m >= k).
fn lstsq(mut a: Vec<Vec<f64>>, mut b: Vec<f64>) -> Option<Vec<f64>> {
    let m = a.len();
    let k = a.first()?.len();
    if m < k {
        return None;
    }
    for j in 0..k {
        let norm = (j..m).map(|i| a[i][j] * a[i][j]).sum::<f64>().sqrt();
        if norm == 0.0 {
            return None;
        }
        let alpha = if a[j][j] > 0.0 { -norm } else { norm };
        let mut v: Vec<f64> = (j..m).map(|i| a[i][j]).collect();
        v[0] -= alpha;
        let vnorm2: f64 = v.iter().map(|x| x * x).sum();
        if vnorm2 == 0.0 {
            continue;
        }
        for col in j..k {
            let dot: f64 = (j..m).map(|i| v[i - j] * a[i][col]).sum();
            let s = 2.0 * dot / vnorm2;
            for i in j..m {
                a[i][col] -= s * v[i - j];
            }
        }
        let dot: f64 = (j..m).map(|i| v[i - j] * b[i]).sum();
        let s = 2.0 * dot / vnorm2;
        for i in j..m {
            b[i] -= s * v[i - j];
        }
    }
    // Back substitution on the upper triangle.
    let mut c = vec![0.0; k];
    for j in (0..k).rev() {
        let mut s = b[j];
        for col in j + 1..k {
            s -= a[j][col] * c[col];
        }
        if a[j][j] == 0.0 {
            return None;
        }
        c[j] = s / a[j][j];
    }
    Some(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exp_fit_reaches_1e3_by_degree_7() {
        let f = |x: f64| x.exp();
        let fit = (3..=7).map(|n| fit_poly(&f, -4.41, 0.21, n, 0.0).unwrap()).find(|f| f.max_rel_err <= 1e-3).unwrap();
        assert!(fit.degree <= 7, "{fit:?}");
        assert!((eval_f32(&fit, -2.0) as f64 - (-2.0f64).exp()).abs() < 1e-3 * (-2.0f64).exp());
    }

    #[test]
    fn degree_parsing() {
        assert_eq!(parse_degree("3..7").unwrap(), (3, 7));
        assert_eq!(parse_degree("5").unwrap(), (5, 5));
        assert!(parse_degree("0..3").is_err());
        assert!(parse_degree("7..3").is_err());
    }
}
