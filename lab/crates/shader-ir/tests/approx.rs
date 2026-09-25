//! M4 `approx` tests: polynomial replacement of GLSL.std.450 transcendentals at listed sites
//! within the requested relative error (f32 interpreter), skips with reasons, and the corpus
//! sites with ranges from `eval --profile`.

mod common;

use common::{compile_src, Run, PRELUDE, SPIRV_VAL};
use shader_ir::analysis::Range;
use shader_ir::interp::{EvalOutput, Filter, Mode};
use shader_ir::lift::Lifted;
use shader_ir::npy::Image;
use shader_ir::passes::{self, approx, glsl_op, id_op, Class, EditOp};
use spirv::{GLOp, StorageClass};
use std::collections::BTreeMap;
use std::path::Path;

const SPV_DIR: &str = "/home/rudybear/sources/shaderopt/lab/build/spv";

macro_rules! compile_or_skip {
    ($name:expr, $src:expr) => {
        match compile_src($name, &format!("{PRELUDE}{}", $src), false) {
            Some(w) => w,
            None => return,
        }
    };
}

fn validate_with_tool(words: &[u32]) {
    if !Path::new(SPIRV_VAL).is_file() {
        return;
    }
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let p = common::tmp_dir().join(format!("approx_val_{}_{}.spv", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    std::fs::write(&p, shader_ir::bytes_from_words(words)).unwrap();
    let st = std::process::Command::new(SPIRV_VAL).arg(&p).output().expect("run spirv-val");
    assert!(st.status.success(), "spirv-val rejected the approximated module: {}", String::from_utf8_lossy(&st.stderr));
}

/// `(site id, operand id)` of every GLSL.std.450 `op` in the module.
fn sites(words: &[u32], op: GLOp) -> Vec<(u32, u32)> {
    let l = Lifted::load(words).unwrap();
    let mut out = Vec::new();
    for f in &l.module.functions {
        for b in &f.blocks {
            for inst in &b.instructions {
                if glsl_op(&l, inst) == Some(op) {
                    out.push((inst.result_id.unwrap(), id_op(inst, 2).unwrap()));
                }
            }
        }
    }
    out
}

fn range(min: f64, max: f64) -> Range {
    Range { min, max, nan: 0, inf: 0, samples: 4096 }
}

fn do_approx(words: &[u32], ranges: &BTreeMap<u32, Range>, site_ids: &[u32], degree: (u32, u32), max_rel_err: f64) -> (Vec<u32>, Vec<EditOp>, Vec<approx::SiteReport>) {
    let mut l = Lifted::load(words).unwrap();
    let opts = approx::ApproxOpts { sites: site_ids.to_vec(), degree, max_rel_err };
    let mut ops = Vec::new();
    let reports = approx::run(&mut l, ranges, &opts, &mut ops).unwrap_or_else(|e| panic!("approx: {e:#}"));
    let out = l.assemble();
    passes::validate(&out).unwrap_or_else(|e| panic!("in-process validation failed: {e:#}"));
    validate_with_tool(&out);
    for r in &reports {
        eprintln!("  %{} {} {} range={:?} degree={:?} err={:?} {}", r.id, r.op, r.ty, r.range, r.degree, r.max_rel_err, r.status);
    }
    (out, ops, reports)
}

fn gradient(w: usize, h: usize) -> Image {
    let mut img = Image::new(w, h);
    for y in 0..h {
        for x in 0..w {
            img.set_texel(x, y, [x as f32 / (w - 1).max(1) as f32, y as f32 / (h - 1).max(1) as f32, 0.5, 1.0]);
        }
    }
    img
}

/// Maximum relative difference `|a - b| / |b|` over the pixels accepted by `keep` (all
/// channels, `b` the original).
fn max_rel_diff(a: &EvalOutput, b: &EvalOutput, keep: &dyn Fn(usize, usize) -> bool) -> f64 {
    let (ia, ib) = (&a.outputs[&0], &b.outputs[&0]);
    let mut worst: f64 = 0.0;
    for y in 0..ia.height {
        for x in 0..ia.width {
            if !keep(x, y) {
                continue;
            }
            let (ta, tb) = (ia.texel(x, y), ib.texel(x, y));
            for c in 0..4 {
                let d = (ta[c] as f64 - tb[c] as f64).abs() / (tb[c] as f64).abs().max(1e-30);
                assert!(d.is_finite(), "pixel ({x},{y}) channel {c}: {} vs {}", ta[c], tb[c]);
                worst = worst.max(d);
            }
        }
    }
    worst
}

fn histogram(words: &[u32], key: &str) -> usize {
    Lifted::load(words).unwrap().opcode_histogram().iter().find(|(n, _)| n == key).map(|x| x.1).unwrap_or(0)
}

const EXP_SCALAR: &str = "void main() { o = vec4(exp(-uv.x * 4.0)); }";

#[test]
fn approx_exp_scalar_within_rel_err() {
    let w0 = compile_or_skip!("approx_exp", EXP_SCALAR);
    let s = sites(&w0, GLOp::Exp);
    assert_eq!(s.len(), 1);
    let (site, x) = s[0];
    let ranges = BTreeMap::from([(x, range(-4.2, 0.0))]);
    let (w1, ops, reports) = do_approx(&w0, &ranges, &[site], (3, 7), 1e-3);
    assert_eq!(ops.len(), 1, "{ops:#?}");
    assert_eq!(ops[0].class, Class::Lossy);
    assert_eq!(ops[0].target, site);
    assert_eq!(ops[0].replaced_by, Some(site));
    assert!(ops[0].detail.starts_with("Exp -> degree-") && ops[0].detail.contains("polynomial on [-4.41, 0.21], max rel err"), "{}", ops[0].detail);
    assert!(reports[0].replaced());
    let degree = reports[0].degree.unwrap();
    assert!(reports[0].max_rel_err.unwrap() <= 1e-3);
    // Horner: one Fma for t, one per coefficient below the leading one; the Exp is gone.
    assert_eq!(histogram(&w1, "ExtInst:Exp"), 0);
    assert_eq!(histogram(&w1, "ExtInst:Fma"), degree as usize + 1);
    let a = Run::new(64, 64, Mode::F32).eval_ok(&w1);
    let b = Run::new(64, 64, Mode::F32).eval_ok(&w0);
    let worst = max_rel_diff(&a, &b, &|_, _| true);
    eprintln!("exp scalar: degree {degree}, fit err {:.2e}, pixel err {worst:.2e}", reports[0].max_rel_err.unwrap());
    // The fit bound is measured against f64 exp on 4096 points; the original is f32 exp
    // (<= 1 ULP), hence the 1e-6 slack.
    assert!(worst <= 1e-3 + 1e-6, "max relative difference {worst}");
}

#[test]
fn approx_range_too_wide_is_skipped() {
    let w0 = compile_or_skip!("approx_wide", EXP_SCALAR);
    let (site, x) = sites(&w0, GLOp::Exp)[0];
    let ranges = BTreeMap::from([(x, range(-40.0, 0.0))]);
    let (w1, ops, reports) = do_approx(&w0, &ranges, &[site], (3, 7), 1e-3);
    assert!(ops.is_empty(), "{ops:#?}");
    assert!(!reports[0].replaced());
    assert!(reports[0].status.starts_with("no degree in 3..7 reaches max rel err 1.0e-3 on [-42.0, 2.0]: best degree "), "{}", reports[0].status);
    assert_eq!(w1[5..], w0[5..], "body must be untouched");
    // A site with no range, and an id that is not a transcendental, are skipped with reasons.
    let (_, ops, reports) = do_approx(&w0, &BTreeMap::new(), &[site, x], (3, 7), 1e-3);
    assert!(ops.is_empty());
    assert!(reports[0].status.contains("has no range"), "{}", reports[0].status);
    assert!(reports[1].status.contains("not a GLSL.std.450"), "{}", reports[1].status);
}

#[test]
fn approx_pow_constant_exponent() {
    // glslang folds 1.0 / 2.2 to a constant exponent.
    let w0 = compile_or_skip!("approx_pow", "void main() { o = vec4(pow(uv.x, 1.0 / 2.2)); }");
    let s = sites(&w0, GLOp::Pow);
    assert_eq!(s.len(), 1);
    let (site, x) = s[0];
    // On [0.25, 1] a low degree suffices ...
    let ranges = BTreeMap::from([(x, range(0.25, 1.0))]);
    let (w1, ops, reports) = do_approx(&w0, &ranges, &[site], (3, 7), 1e-3);
    assert_eq!(ops.len(), 1, "{ops:#?}");
    assert!(ops[0].detail.starts_with("Pow(x, 0.4545) -> degree-"), "{}", ops[0].detail);
    assert_eq!(histogram(&w1, "ExtInst:Pow"), 0);
    let a = Run::new(64, 64, Mode::F32).eval_ok(&w1);
    let b = Run::new(64, 64, Mode::F32).eval_ok(&w0);
    // Only pixels inside the fitted range are guaranteed (no guard is emitted).
    let worst = max_rel_diff(&a, &b, &|x, _| (x as f64 + 0.5) / 64.0 >= 0.25);
    eprintln!("pow 1/2.2 on [0.25, 1]: degree {}, fit err {:.2e}, pixel err {worst:.2e}", reports[0].degree.unwrap(), reports[0].max_rel_err.unwrap());
    assert!(worst <= 1e-3 + 1e-6, "max relative difference {worst}");
    // ... but x^0.45 has an infinite derivative at 0: on [0, 1] no degree <= 7 reaches 1e-3
    // relative error, so the site is skipped with the best error as the reason.
    let ranges = BTreeMap::from([(x, range(0.0, 1.0))]);
    let (_, ops, reports) = do_approx(&w0, &ranges, &[site], (3, 7), 1e-3);
    assert!(ops.is_empty());
    assert!(reports[0].status.starts_with("no degree in 3..7 reaches"), "{}", reports[0].status);
    assert_eq!(reports[0].range, Some((0.0, 1.05)));
    // A non-constant exponent is not approximated.
    let w2 = compile_or_skip!(
        "approx_pow_u",
        "layout(set = 0, binding = 1, std140) uniform Params { float g; vec3 pad0; } u; void main() { o = vec4(pow(uv.x, u.g)); }"
    );
    let (site2, x2) = sites(&w2, GLOp::Pow)[0];
    let (_, ops, reports) = do_approx(&w2, &BTreeMap::from([(x2, range(0.25, 1.0))]), &[site2], (3, 7), 1e-3);
    assert!(ops.is_empty());
    assert!(reports[0].status.contains("not a constant"), "{}", reports[0].status);
}

#[test]
fn approx_vector_exp_site() {
    let w0 = compile_or_skip!("approx_vexp", "void main() { o = vec4(exp(-vec3(uv.x, uv.y, uv.x * uv.y) * 3.0), 1.0); }");
    let s = sites(&w0, GLOp::Exp);
    assert_eq!(s.len(), 1);
    let (site, x) = s[0];
    let ranges = BTreeMap::from([(x, range(-3.0, 0.0))]);
    let (w1, ops, reports) = do_approx(&w0, &ranges, &[site], (3, 7), 1e-3);
    assert_eq!(ops.len(), 1);
    assert_eq!(reports[0].ty, "vec3");
    let degree = reports[0].degree.unwrap();
    assert_eq!(histogram(&w1, "ExtInst:Fma"), degree as usize + 1);
    // The Fma instructions are vec3-typed (splat constants).
    let l = Lifted::load(&w1).unwrap();
    for f in &l.module.functions {
        for b in &f.blocks {
            for inst in &b.instructions {
                if glsl_op(&l, inst) == Some(GLOp::Fma) {
                    assert_eq!(l.type_name(inst.result_type.unwrap()), "vec3");
                }
            }
        }
    }
    let a = Run::new(64, 64, Mode::F32).eval_ok(&w1);
    let b = Run::new(64, 64, Mode::F32).eval_ok(&w0);
    let worst = max_rel_diff(&a, &b, &|_, _| true);
    eprintln!("exp vec3: degree {degree}, fit err {:.2e}, pixel err {worst:.2e}", reports[0].max_rel_err.unwrap());
    assert!(worst <= 1e-3 + 1e-6, "max relative difference {worst}");
}

#[test]
fn approx_corpus_with_profiled_ranges() {
    for (name, uniforms) in [
        ("gaussian_blur_h", vec![("texel", "[0.015625, 0.015625]"), ("sigma", "1.5")]),
        ("tonemap_aces", vec![("white_balance", "[1.0, 0.95, 0.9, 0.0]"), ("exposure", "1.4"), ("gamma", "2.2"), ("contrast", "1.1")]),
    ] {
        let p = Path::new(SPV_DIR).join(format!("{name}.spv"));
        if !p.is_file() {
            eprintln!("SKIP: no corpus module {}", p.display());
            continue;
        }
        let w0 = shader_ir::read_spv(&p).unwrap();
        let l = Lifted::load(&w0).unwrap();
        let samplers: Vec<String> = l
            .variables
            .iter()
            .filter(|v| v.storage == StorageClass::UniformConstant)
            .map(|v| v.name.clone().unwrap_or_else(|| format!("binding{}", v.binding.unwrap_or(0))))
            .collect();
        let mk = |profile: bool| {
            let mut r = Run::new(64, 64, Mode::F32);
            for (n, v) in &uniforms {
                r = r.uniform(n, v);
            }
            for s in &samplers {
                r = r.sampler(s, gradient(32, 32), Filter::Linear);
            }
            r.cfg.profile = profile;
            r
        };
        // `eval --profile` on a gradient input.
        let base = mk(true).eval_ok(&w0);
        let ranges = base.ranges.clone().unwrap();
        let mut site_ids: Vec<u32> = Vec::new();
        for op in [GLOp::Exp, GLOp::Pow] {
            site_ids.extend(sites(&w0, op).iter().map(|s| s.0));
        }
        assert!(!site_ids.is_empty(), "{name}: no Exp/Pow sites");
        for degree in [(3, 7), (3, 12)] {
            eprintln!("{name}: approx --degree {}..{} --max-rel-err 1e-3, sites {site_ids:?}", degree.0, degree.1);
            let (w1, ops, reports) = do_approx(&w0, &ranges, &site_ids, degree, 1e-3);
            let replaced = reports.iter().filter(|r| r.replaced()).count();
            assert_eq!(ops.len(), replaced);
            for o in &ops {
                assert_eq!(o.class, Class::Lossy);
                eprintln!("  op: {}", o.detail);
            }
            if replaced > 0 {
                let a = mk(false).eval_ok(&w1);
                let worst = max_rel_diff(&a, &base, &|_, _| true);
                eprintln!("  {name}: pixel max rel diff vs original {worst:.2e}");
                assert!(worst <= 3e-3, "{name}: {worst}");
            }
        }
    }
}
