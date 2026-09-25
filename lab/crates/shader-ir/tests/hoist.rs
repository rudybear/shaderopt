//! M4 `hoist` tests: uniform-rate subtrees become block members whose values the interpreter
//! dumps from the input module; the hoisted module fed those values is bit-identical (f32).

mod common;

use common::{compile_src, Run, PRELUDE, SPIRV_VAL};
use shader_ir::interp::{dump, EvalOutput, Filter, Mode};
use shader_ir::lift::{Lifted, Type};
use shader_ir::npy::Image;
use shader_ir::passes::{self, hoist, Class, EditOp, Opts};
use spirv::StorageClass;
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
    let p = common::tmp_dir().join(format!("hoist_val_{}_{}.spv", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    std::fs::write(&p, shader_ir::bytes_from_words(words)).unwrap();
    let st = std::process::Command::new(SPIRV_VAL).arg(&p).output().expect("run spirv-val");
    assert!(st.status.success(), "spirv-val rejected the hoisted module: {}", String::from_utf8_lossy(&st.stderr));
}

fn rewrite(words: &[u32], passes: &str) -> Vec<u32> {
    let mut l = Lifted::load(words).unwrap();
    let list: Vec<String> = passes.split(',').map(String::from).collect();
    let mut ops = Vec::new();
    passes::run_pipeline(&mut l, &list, &mut ops, &Opts::default()).unwrap_or_else(|e| panic!("pipeline: {e:#}"));
    l.assemble()
}

/// `hoist` + `dce` (what the CLI does), validated in-process and with spirv-val.
fn do_hoist(words: &[u32], min_ops: usize) -> (Vec<u32>, Vec<EditOp>, hoist::HoistReport) {
    let mut l = Lifted::load(words).unwrap();
    let mut ops = Vec::new();
    let report = hoist::run(&mut l, &mut ops, min_ops).unwrap_or_else(|e| panic!("hoist: {e:#}"));
    if !report.entries.is_empty() {
        passes::run_pass("dce", &mut l, &mut ops, &Opts::default()).unwrap();
    }
    let out = l.assemble();
    passes::validate(&out).unwrap_or_else(|e| panic!("in-process validation failed: {e:#}"));
    validate_with_tool(&out);
    (out, ops, report)
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

fn assert_bit_identical(a: &EvalOutput, b: &EvalOutput, what: &str) {
    assert_eq!(a.discarded_pixels, b.discarded_pixels, "{what}: discard count");
    for (loc, ia) in &a.outputs {
        let ib = &b.outputs[loc];
        for y in 0..ia.height {
            for x in 0..ia.width {
                let (ta, tb) = (ia.texel(x, y), ib.texel(x, y));
                for c in 0..4 {
                    assert_eq!(ta[c].to_bits(), tb[c].to_bits(), "{what}: pixel ({x},{y}) channel {c}: {} vs {}", ta[c], tb[c]);
                }
            }
        }
    }
}

fn run_with(w: usize, h: usize, uniforms: &[(String, String)], samplers: &[String]) -> Run {
    let mut r = Run::new(w, h, Mode::F32);
    for (n, v) in uniforms {
        r = r.uniform(n, v);
    }
    for s in samplers {
        r = r.sampler(s, gradient(8, 8), Filter::Linear);
    }
    r
}

fn sampler_names(words: &[u32]) -> Vec<String> {
    let l = Lifted::load(words).unwrap();
    l.variables
        .iter()
        .filter(|v| v.storage == StorageClass::UniformConstant)
        .map(|v| v.name.clone().unwrap_or_else(|| format!("binding{}", v.binding.unwrap_or(0))))
        .collect()
}

/// Dumps the plan's source ids from `input` at 2x2 (the documented recipe) and returns the
/// `--uniform h_<id>=[...]` pairs.
fn dumped_uniforms(input: &[u32], report: &hoist::HoistReport, uniforms: &[(String, String)], samplers: &[String]) -> Vec<(String, String)> {
    let l = Lifted::load(input).unwrap();
    let cfg = run_with(2, 2, uniforms, samplers).cfg;
    let ids: Vec<u32> = report.entries.iter().map(|e| e.source_id).collect();
    let d = dump::dump_values(&l, &cfg, &ids).unwrap_or_else(|e| panic!("dump: {e:#}"));
    if !d.never_executed.is_empty() {
        eprintln!("never executed at 2x2 (zero-filled): {:?}", d.never_executed);
    }
    // The JSON round trip is part of the contract.
    let json = dump::dump_json(&d);
    let text = serde_json::to_string(&json).unwrap();
    let mut back: BTreeMap<String, serde_json::Value> = serde_json::from_str(&text).unwrap();
    back.remove("_never_executed");
    let back: BTreeMap<String, Vec<f64>> = back.into_iter().map(|(k, v)| (k, serde_json::from_value(v).unwrap())).collect();
    report
        .entries
        .iter()
        .map(|e| {
            let vals = &back[&e.source_id.to_string()];
            (e.member.clone(), format!("[{}]", vals.iter().map(|v| format!("{v}")).collect::<Vec<_>>().join(",")))
        })
        .collect()
}

/// Bit-identical f32 evaluation of `hoisted` (with the dumped member values) against `input`
/// and `original` on `w x h`.
fn check_identical(original: &[u32], input: &[u32], hoisted: &[u32], report: &hoist::HoistReport, uniforms: &[(String, String)], w: usize, h: usize, what: &str) {
    let samplers = sampler_names(input);
    let mut all = uniforms.to_vec();
    all.extend(dumped_uniforms(input, report, uniforms, &samplers));
    let a = run_with(w, h, uniforms, &samplers).eval_ok(input);
    let b = run_with(w, h, &all, &samplers).eval_ok(hoisted);
    assert_bit_identical(&a, &b, &format!("{what}: hoisted vs input"));
    let o = run_with(w, h, uniforms, &samplers).eval_ok(original);
    assert_bit_identical(&o, &b, &format!("{what}: hoisted vs original"));
}

fn u(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
}

fn histogram(words: &[u32], key: &str) -> usize {
    Lifted::load(words).unwrap().opcode_histogram().iter().find(|(n, _)| n == key).map(|x| x.1).unwrap_or(0)
}

fn member_layout(words: &[u32]) -> Vec<(String, String, u32)> {
    let l = Lifted::load(words).unwrap();
    let v = l.variables.iter().find(|v| v.storage == StorageClass::Uniform && v.block).expect("block");
    let Type::Struct { members } = l.ty(v.pointee).unwrap() else { panic!() };
    members
        .iter()
        .enumerate()
        .map(|(m, t)| {
            (
                l.member_name(v.pointee, m as u32).unwrap_or("?").to_string(),
                l.type_name(*t),
                l.member_decoration_u32(v.pointee, m as u32, spirv::Decoration::Offset).unwrap(),
            )
        })
        .collect()
}

const GAUSS: &str = "layout(set = 0, binding = 0) uniform sampler2D u_src;
layout(set = 0, binding = 1, std140) uniform Params { vec2 texel; float sigma; float pad0; } u;
void main() { vec3 acc = vec3(0.0);
  for (int i = -4; i <= 4; ++i) { float x = float(i); float w = exp(-0.5 * x * x / (u.sigma * u.sigma));
    acc += texture(u_src, uv + vec2(x * u.texel.x, 0.0)).rgb * w; }
  o = vec4(acc, 1.0); }";

#[test]
fn hoist_unrolled_gaussian_weights() {
    let w0 = compile_or_skip!("hoist_gauss", GAUSS);
    let w1 = rewrite(&w0, "fold,dce,cse,ident,unroll");
    let (w2, ops, report) = do_hoist(&w1, 2);
    // 9 weights (cse does not merge the symmetric ones: the induction constants reach the
    // math through Function variables, see README), each exp / div / 3 muls = 5 ops.
    assert_eq!(report.entries.len(), 9, "{:#?}", report.entries);
    assert_eq!(report.block.as_deref(), Some("u"));
    for e in &report.entries {
        assert_eq!(e.ty, "float");
        assert_eq!(e.ops, 5, "{e:?}");
        assert_eq!(e.depends_on, vec!["sigma".to_string()], "{e:?}");
        assert!(e.expr.starts_with("exp(") && e.expr.contains("(sigma * sigma)"), "{}", e.expr);
        assert!(e.member.starts_with("h_"));
    }
    let hoists: Vec<&EditOp> = ops.iter().filter(|o| o.pass == "hoist" && o.replaced_by.is_some()).collect();
    assert_eq!(hoists.len(), 9);
    for o in &hoists {
        assert_eq!(o.class, Class::Exact);
        assert!(o.detail.starts_with("hoisted 5 instructions into Params.h_") && o.detail.ends_with("(float)"), "{}", o.detail);
    }
    // Layout: existing members untouched, new ones appended at std140 offsets 16, 20, ...
    let layout = member_layout(&w2);
    assert_eq!(layout[..3], [("texel".to_string(), "vec2".to_string(), 0), ("sigma".to_string(), "float".to_string(), 8), ("pad0".to_string(), "float".to_string(), 12)]);
    for (k, e) in report.entries.iter().enumerate() {
        assert_eq!(layout[3 + k], (e.member.clone(), "float".to_string(), 16 + 4 * k as u32));
        assert_eq!(e.offset, 16 + 4 * k as u32);
    }
    assert_eq!(report.bytes_added, 36);
    // The weight math is gone from the shader.
    assert_eq!(histogram(&w2, "ExtInst:Exp"), 0);
    assert_eq!(histogram(&w2, "OpFDiv"), 0);
    let (n1, n2) = (passes::instruction_count(&Lifted::load(&w1).unwrap()), passes::instruction_count(&Lifted::load(&w2).unwrap()));
    assert!(n2 + 40 < n1, "instructions {n1} -> {n2}");
    eprintln!("gaussian test shader: instructions {n1} -> {n2}, {} ops", ops.len());
    let uniforms = u(&[("texel", "[0.125, 0.125]"), ("sigma", "1.7")]);
    check_identical(&w0, &w1, &w2, &report, &uniforms, 16, 16, "gaussian");
}

#[test]
fn hoist_min_ops_and_uniform_only_output() {
    // vec4(a*b*c, 0, 0, 1) is uniform and stored to the output: maximal (the store is the
    // consumer), 2 ops.
    let w0 = compile_or_skip!(
        "hoist_out",
        "layout(set = 0, binding = 1, std140) uniform Params { float a; float b; float c; float pad0; } u;
         void main() { o = vec4(u.a * u.b * u.c, 0.0, 0.0, 1.0); }"
    );
    let (w2, ops, report) = do_hoist(&w0, 2);
    assert_eq!(report.entries.len(), 1, "{:#?}", report.entries);
    let e = &report.entries[0];
    assert_eq!(e.ty, "vec4");
    assert_eq!(e.ops, 2);
    assert_eq!(e.expr, "vec4(((a * b) * c), 0.0, 0.0, 1.0)");
    assert_eq!(e.depends_on, vec!["a".to_string(), "b".to_string(), "c".to_string()]);
    assert_eq!(e.offset, 16);
    assert!(ops.iter().any(|o| o.pass == "dce"), "{ops:#?}");
    let uniforms = u(&[("a", "1.5"), ("b", "0.3"), ("c", "7.0")]);
    check_identical(&w0, &w0, &w2, &report, &uniforms, 4, 4, "uniform output");
    // With --min-ops 3 the 2-op value stays.
    let (_, ops3, report3) = do_hoist(&w0, 3);
    assert!(report3.entries.is_empty() && ops3.is_empty(), "{:#?}", report3.entries);
}

#[test]
fn hoist_nothing_uniform_leaves_module_alone() {
    let w0 = compile_or_skip!(
        "hoist_none",
        "layout(set = 0, binding = 1, std140) uniform Params { float scale; vec3 pad0; } u;
         void main() { o = vec4(uv * u.scale, 0.0, 1.0); }"
    );
    let (w2, ops, report) = do_hoist(&w0, 2);
    assert!(report.entries.is_empty(), "{:#?}", report.entries);
    assert_eq!(report.block.as_deref(), Some("u"));
    assert!(ops.is_empty(), "{ops:#?}");
    assert_eq!(w2[5..], w0[5..], "body must be untouched");
}

#[test]
fn hoist_without_block_reports_no_block() {
    let w0 = compile_or_skip!("hoist_noblock", "void main() { o = vec4(uv, 0.0, 1.0); }");
    let (w2, ops, report) = do_hoist(&w0, 1);
    assert!(report.block.is_none());
    assert!(report.entries.is_empty() && ops.is_empty());
    assert_eq!(w2[5..], w0[5..]);
}

#[test]
fn hoist_does_not_forward_loop_carried_or_conditional_stores() {
    // `s` is loop-carried (not unrolled: the trip count comes from a uniform): the load inside
    // the loop has two reaching definitions, so `s * a + b` is not hoistable. `t` is stored
    // conditionally: its load is not forwarded either, but the stored value `a * b * 2.0` is a
    // uniform computation whose consumer (the store) is not transparent, so it is hoisted and
    // the conditional store keeps the branch. `k` is straight-line.
    let w0 = compile_or_skip!(
        "hoist_loops",
        "layout(set = 0, binding = 1, std140) uniform Params { float a; float b; int n; float pad0; } u;
         void main() {
           float s = 1.0; for (int i = 0; i < u.n; ++i) { s = s * u.a + u.b; }
           float t = 0.0; if (u.a > 0.5) { t = u.a * u.b * 2.0; }
           float k = sin(u.a) * cos(u.b);
           o = vec4(uv.x * s, uv.y * t, uv.x * k, 1.0); }"
    );
    let (w2, _ops, report) = do_hoist(&w0, 2);
    let exprs: Vec<&str> = report.entries.iter().map(|e| e.expr.as_str()).collect();
    assert_eq!(exprs, ["((a * b) * 2.0)", "(sin(a) * cos(b))"], "{:#?}", report.entries);
    assert_eq!(histogram(&w2, "OpLoopMerge"), 1);
    assert_eq!(histogram(&w2, "OpSelectionMerge"), 1);
    for uniforms in [u(&[("a", "0.75"), ("b", "0.2"), ("n", "3")]), u(&[("a", "0.25"), ("b", "-0.2"), ("n", "0")])] {
        check_identical(&w0, &w0, &w2, &report, &uniforms, 4, 4, "loops");
    }
}

#[test]
fn hoist_corpus_color_grade_and_gaussian() {
    for (name, uniforms, pre) in [
        (
            "color_grade",
            u(&[
                ("lift", "[0.02, 0.01, 0.0, 0.0]"),
                ("gamma", "[1.1, 1.0, 0.9, 0.0]"),
                ("gain", "[1.2, 1.0, 0.8, 0.0]"),
                ("mat_r", "[1.0, 0.1, 0.0, 0.0]"),
                ("mat_g", "[0.0, 1.0, 0.1, 0.0]"),
                ("mat_b", "[0.1, 0.0, 1.0, 0.0]"),
                ("saturation", "1.3"),
                ("temperature", "0.4"),
            ]),
            "fold,dce,cse,ident",
        ),
        ("gaussian_blur_h", u(&[("texel", "[0.015625, 0.015625]"), ("sigma", "1.5")]), "fold,dce,cse,ident,unroll"),
    ] {
        let p = Path::new(SPV_DIR).join(format!("{name}.spv"));
        if !p.is_file() {
            eprintln!("SKIP: no corpus module {}", p.display());
            continue;
        }
        let w0 = shader_ir::read_spv(&p).unwrap();
        let w1 = rewrite(&w0, pre);
        let (w2, ops, report) = do_hoist(&w1, 2);
        assert!(!report.entries.is_empty(), "{name}: nothing hoisted");
        let (n0, n1, n2) = (
            passes::instruction_count(&Lifted::load(&w0).unwrap()),
            passes::instruction_count(&Lifted::load(&w1).unwrap()),
            passes::instruction_count(&Lifted::load(&w2).unwrap()),
        );
        eprintln!(
            "{name}: {} members (+{} bytes std140), instructions original {n0}, after `{pre}` {n1}, after hoist+dce {n2}; {} ops, {} dead variables",
            report.entries.len(),
            report.bytes_added,
            ops.len(),
            report.dead_variables
        );
        for e in &report.entries {
            let mut expr = e.expr.clone();
            if expr.len() > 100 {
                expr.truncate(100);
                expr.push_str("...");
            }
            eprintln!("  {} {} offset={} ops={} deps={:?} {expr}", e.member, e.ty, e.offset, e.ops, e.depends_on);
        }
        check_identical(&w0, &w1, &w2, &report, &uniforms, 32, 32, name);
    }
}
