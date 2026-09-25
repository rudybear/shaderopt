//! M2 rewrite-pass tests: each pass fires on a tiny GLSL shader (count and class of ops), the
//! output validates, and the interpreter agrees with the original (bit-identical in f64 mode
//! for `exact` passes, <= 2 ULP in f32 mode for `ulp` passes). The full pipeline runs on every
//! corpus module in `lab/build/spv`.

mod common;

use common::{compile_src, Run, PRELUDE, SPIRV_VAL};
use shader_ir::interp::{EvalOutput, Filter, Mode};
use shader_ir::lift::{Lifted, Type};
use shader_ir::npy::Image;
use shader_ir::passes::{self, Class, EditOp, Opts};
use spirv::StorageClass;
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

fn rewrite(words: &[u32], passes: &str, opts: &Opts) -> (Vec<u32>, Vec<EditOp>, Vec<(String, usize)>) {
    let mut l = Lifted::load(words).unwrap();
    let bound0 = l.bound;
    let list: Vec<String> = passes.split(',').map(String::from).collect();
    let mut ops = Vec::new();
    let counts = passes::run_pipeline(&mut l, &list, &mut ops, opts).unwrap_or_else(|e| panic!("pipeline: {e:#}"));
    let out = l.assemble();
    passes::validate(&out).unwrap_or_else(|e| panic!("in-process validation failed: {e:#}"));
    validate_with_tool(&out);
    // Header bound covers every id; untouched instructions keep their ids (spot check: the
    // entry point function id is unchanged).
    assert!(out[3] >= bound0, "bound shrank: {} -> {}", bound0, out[3]);
    assert_eq!(l.entry().unwrap().function, Lifted::load(words).unwrap().entry().unwrap().function);
    (out, ops, counts)
}

/// spirv-val on a per-call temp file (the shared helper reuses one file name, which races
/// between parallel tests).
fn validate_with_tool(words: &[u32]) {
    if !Path::new(SPIRV_VAL).is_file() {
        return;
    }
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let p = common::tmp_dir().join(format!("rw_val_{}_{}.spv", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    std::fs::write(&p, shader_ir::bytes_from_words(words)).unwrap();
    let st = std::process::Command::new(SPIRV_VAL).arg(&p).output().expect("run spirv-val");
    assert!(st.status.success(), "spirv-val rejected the rewritten module: {}", String::from_utf8_lossy(&st.stderr));
}

fn count(ops: &[EditOp], pass: &str) -> usize {
    ops.iter().filter(|o| o.pass == pass).count()
}

fn all_class(ops: &[EditOp], pass: &str, class: Class) {
    for o in ops.iter().filter(|o| o.pass == pass) {
        assert_eq!(o.class, class, "{pass} op {:?} has class {:?}", o.detail, o.class);
    }
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
    assert_eq!(a.outputs.keys().collect::<Vec<_>>(), b.outputs.keys().collect::<Vec<_>>());
    assert_eq!(a.discarded_pixels, b.discarded_pixels, "{what}: discard count");
    for (loc, ia) in &a.outputs {
        let ib = &b.outputs[loc];
        for y in 0..ia.height {
            for x in 0..ia.width {
                let (ta, tb) = (ia.texel(x, y), ib.texel(x, y));
                for c in 0..4 {
                    assert_eq!(
                        ta[c].to_bits(),
                        tb[c].to_bits(),
                        "{what}: location {loc} pixel ({x},{y}) channel {c}: {} vs {}",
                        ta[c],
                        tb[c]
                    );
                }
            }
        }
    }
}

fn ulp_distance(a: f32, b: f32) -> u64 {
    if a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan()) {
        return 0;
    }
    if a.is_nan() || b.is_nan() {
        return u64::MAX;
    }
    let key = |x: f32| {
        let i = x.to_bits() as i32;
        (if i < 0 { i32::MIN - i } else { i }) as i64
    };
    (key(a) - key(b)).unsigned_abs()
}

fn assert_within_ulp(a: &EvalOutput, b: &EvalOutput, max: u64, what: &str) -> u64 {
    let mut worst = 0;
    for (loc, ia) in &a.outputs {
        let ib = &b.outputs[loc];
        for y in 0..ia.height {
            for x in 0..ia.width {
                let (ta, tb) = (ia.texel(x, y), ib.texel(x, y));
                for c in 0..4 {
                    let d = ulp_distance(ta[c], tb[c]);
                    assert!(d <= max, "{what}: pixel ({x},{y}) channel {c}: {} vs {} is {d} ULP", ta[c], tb[c]);
                    worst = worst.max(d);
                }
            }
        }
    }
    worst
}

/// f64 evaluation of both modules on 16x16 with a gradient sampler for every sampler and the
/// given uniforms; asserts bit identity.
fn check_exact(orig: &[u32], new: &[u32], uniforms: &[(&str, &str)], what: &str) {
    let mk = || {
        let mut r = Run::new(16, 16, Mode::F64);
        for (n, v) in uniforms {
            r = r.uniform(n, v);
        }
        for name in sampler_names(orig) {
            r = r.sampler(&name, gradient(8, 8), Filter::Linear);
        }
        r
    };
    let a = mk().eval_ok(orig);
    let b = mk().eval_ok(new);
    assert_bit_identical(&a, &b, what);
}

fn check_ulp(orig: &[u32], new: &[u32], uniforms: &[(&str, &str)], max: u64, what: &str) -> u64 {
    let mk = || {
        let mut r = Run::new(16, 16, Mode::F32);
        for (n, v) in uniforms {
            r = r.uniform(n, v);
        }
        for name in sampler_names(orig) {
            r = r.sampler(&name, gradient(8, 8), Filter::Linear);
        }
        r
    };
    let a = mk().eval_ok(orig);
    let b = mk().eval_ok(new);
    assert_within_ulp(&a, &b, max, what)
}

fn sampler_names(words: &[u32]) -> Vec<String> {
    let l = Lifted::load(words).unwrap();
    l.variables
        .iter()
        .filter(|v| v.storage == StorageClass::UniformConstant)
        .map(|v| v.name.clone().unwrap_or_else(|| format!("binding{}", v.binding.unwrap_or(0))))
        .collect()
}

// ---- fold ------------------------------------------------------------------------------------

#[test]
fn fold_exact_after_unroll() {
    // glslang folds literal expressions itself; constants reach SPIR-V through the unrolled
    // induction variable: float(i) * 0.5 and float(i) + 2.0 fold exactly.
    let w = compile_or_skip!(
        "fold_exact",
        "void main() { float s = 0.0; for (int i = 0; i < 4; ++i) { s += (float(i) * 0.5 + 2.0) * uv.x; } o = vec4(s); }"
    );
    let (out, ops, _) = rewrite(&w, "unroll,fold,dce", &Opts::default());
    assert_eq!(count(&ops, "unroll"), 1);
    let folds: Vec<&EditOp> = ops.iter().filter(|o| o.pass == "fold").collect();
    // Per iteration: ConvertSToF, FMul, FAdd (+ the OpIAdd of the increment for iterations 0..3).
    assert!(folds.len() >= 12, "fold ops: {:#?}", folds);
    all_class(&ops, "fold", Class::Exact);
    assert!(folds.iter().any(|o| o.detail.contains("OpConvertSToF") && o.detail.contains("-> OpConstant 3.0")), "{folds:#?}");
    assert!(folds.iter().any(|o| o.detail.contains("OpFMul") && o.detail.contains("-> OpConstant 1.5")), "{folds:#?}");
    check_exact(&w, &out, &[], "fold exact");
}

#[test]
fn fold_transcendental_is_ulp() {
    let w = compile_or_skip!(
        "fold_ulp",
        "void main() { float s = 0.0; for (int i = 1; i < 4; ++i) { s += exp(float(i)) * uv.x + sqrt(float(i)); } o = vec4(s); }"
    );
    let (out, ops, _) = rewrite(&w, "unroll,fold,dce", &Opts::default());
    let exp_folds: Vec<&EditOp> = ops.iter().filter(|o| o.pass == "fold" && o.detail.contains("Exp")).collect();
    assert_eq!(exp_folds.len(), 3, "{exp_folds:#?}");
    for o in &exp_folds {
        assert_eq!(o.class, Class::Ulp);
    }
    let sqrt_folds = ops.iter().filter(|o| o.pass == "fold" && o.detail.contains("Sqrt")).count();
    assert_eq!(sqrt_folds, 3);
    // exp(1) + sqrt(2) rounds in f32: the FAdd fold is ulp too; the exp(3) * x product is not
    // folded (x is per-pixel).
    let worst = check_ulp(&w, &out, &[], 2, "fold ulp");
    eprintln!("fold ulp: worst {worst} ULP");
}

#[test]
fn fold_composites_and_select() {
    // Composite construct/extract of constants and OpSelect on a constant condition appear
    // after unrolling `vec2(float(i), 1.0).x` and `(i > 1) ? a : b`.
    let w = compile_or_skip!(
        "fold_comp",
        "void main() { float s = 0.0; for (int i = 0; i < 3; ++i) { vec2 v = vec2(float(i), 1.0); s += ((i > 1) ? uv.x : uv.y) * v.x; } o = vec4(s); }"
    );
    let (out, ops, _) = rewrite(&w, "unroll,fold,dce", &Opts::default());
    assert!(ops.iter().any(|o| o.pass == "fold" && o.detail.contains("OpCompositeConstruct") && o.detail.contains("OpConstantComposite")), "{ops:#?}");
    assert!(ops.iter().any(|o| o.pass == "fold" && o.detail.contains("OpSGreaterThan")), "{ops:#?}");
    all_class(&ops, "fold", Class::Exact);
    check_exact(&w, &out, &[], "fold composites");
}

// ---- dce -------------------------------------------------------------------------------------

#[test]
fn dce_removes_dead_variable_and_math() {
    let w = compile_or_skip!(
        "dce",
        "void main() { float unused = uv.x * 3.0; vec2 dead2; dead2.x = uv.y; o = vec4(uv, 0.0, 1.0); }"
    );
    let (out, ops, _) = rewrite(&w, "dce", &Opts::default());
    let n = count(&ops, "dce");
    assert!(n >= 4, "dce ops: {ops:#?}");
    all_class(&ops, "dce", Class::Exact);
    assert!(ops.iter().any(|o| o.detail.contains("\"unused\" only stored to")), "{ops:#?}");
    assert!(ops.iter().any(|o| o.detail.contains("\"dead2\" only stored to")), "{ops:#?}");
    assert!(ops.iter().any(|o| o.detail.contains("OpFMul") && o.detail.contains("unused")), "{ops:#?}");
    let l = Lifted::load(&out).unwrap();
    assert!(l.name(0).is_none());
    assert!(!l.names.values().any(|n| n == "unused" || n == "dead2"), "names of removed ids must go: {:?}", l.names);
    check_exact(&w, &out, &[], "dce");
}

#[test]
fn dce_keeps_stores_to_outputs_and_kills() {
    let w = compile_or_skip!("dce_keep", "void main() { if (uv.x > 0.5) discard; o = vec4(uv, 0.0, 1.0); }");
    let (out, ops, _) = rewrite(&w, "dce", &Opts::default());
    assert_eq!(count(&ops, "dce"), 0, "{ops:#?}");
    check_exact(&w, &out, &[], "dce keep");
}

// ---- cse -------------------------------------------------------------------------------------

#[test]
fn cse_merges_dominating_duplicates() {
    let w = compile_or_skip!(
        "cse",
        "layout(set = 0, binding = 1) uniform Params { float k; } u;
         void main() { float a = dot(uv, uv) * u.k; float b = dot(uv, uv) * u.k; o = vec4(a + b); }"
    );
    let (out, ops, _) = rewrite(&w, "cse", &Opts::default());
    // Input loads, the uniform access chain + load, OpDot and OpFMul are merged.
    assert!(count(&ops, "cse") >= 5, "cse ops: {ops:#?}");
    all_class(&ops, "cse", Class::Exact);
    assert!(ops.iter().any(|o| o.detail.starts_with("OpDot")), "{ops:#?}");
    assert!(ops.iter().any(|o| o.detail.starts_with("OpLoad")), "{ops:#?}");
    check_exact(&w, &out, &[("k", "1.5")], "cse");
}

#[test]
fn cse_respects_dominance_and_memory() {
    // The two dot() calls live in sibling arms: neither dominates the other, so nothing merges
    // across them; the loads of the Function variable `t` must not merge across the store.
    let w = compile_or_skip!(
        "cse_dom",
        "void main() { float r; float t = uv.x; if (uv.x < 0.5) { r = dot(uv, uv); } else { r = dot(uv, uv) + 1.0; }
                       float p = t; t = 2.0; float q = t; o = vec4(r, p, q, 1.0); }"
    );
    let (out, ops, _) = rewrite(&w, "cse", &Opts::default());
    assert!(!ops.iter().any(|o| o.detail.starts_with("OpDot")), "dot merged across arms: {ops:#?}");
    assert!(!ops.iter().any(|o| o.detail.starts_with("OpLoad %") && o.detail.contains("== ") && o.detail.contains("OpLoad") && o.detail.contains("%t")), "{ops:#?}");
    check_exact(&w, &out, &[], "cse dominance");
    // Bit-identical output means the loads of `t` were not merged (p == uv.x, q == 2).
    let r = Run::new(4, 4, Mode::F64).eval_ok(&out);
    assert_eq!(r.outputs[&0].texel(1, 1)[2], 2.0);
}

// ---- ident -----------------------------------------------------------------------------------

#[test]
fn ident_all_forms() {
    let w = compile_or_skip!(
        "ident",
        "void main() {
           float a = uv.x * 1.0 + 0.0;
           float b = 1.0 * uv.y - 0.0;
           float c = -(-a) / 1.0;
           vec2 d = uv * 1.0;
           vec2 e = uv * vec2(1.0, 1.0);
           int i = int(uv.x * 4.0) * 1 + 0;
           float f = (uv.x < 0.5) ? 2.0 : 2.0;   // glslang emits OpSelect %c %2 %2
           float g = uv.x * 0.0;    // not an identity (NaN/Inf)
           float h = uv.x - uv.x;   // not an identity
           o = vec4(a + b + c + d.x + e.y + float(i) + f, g, h, 1.0); }"
    );
    // `dce` removes the inner negate that -(-x) leaves unused.
    let (out, ops, _) = rewrite(&w, "ident,dce", &Opts::default());
    let n = count(&ops, "ident");
    assert_eq!(n, 11, "ident ops: {ops:#?}");
    all_class(&ops, "ident", Class::Exact);
    for what in ["x*1", "x+0", "1*x", "x-0", "-(-x)", "x/1", "v*1", "select(c,x,x)"] {
        assert!(ops.iter().any(|o| o.detail.contains(&format!("[{what}"))), "missing {what}: {ops:#?}");
    }
    let l = Lifted::load(&out).unwrap();
    let h = l.opcode_histogram();
    let get = |k: &str| h.iter().find(|(n, _)| n == k).map(|x| x.1).unwrap_or(0);
    assert_eq!(get("OpFNegate"), 0);
    assert_eq!(get("OpFDiv"), 0);
    assert_eq!(get("OpVectorTimesScalar"), 0);
    // x*0 and x-x survive.
    assert_eq!(get("OpFSub"), 1);
    assert!(get("OpFMul") >= 2);
    check_exact(&w, &out, &[], "ident");
}

// ---- unroll ----------------------------------------------------------------------------------

#[test]
fn unroll_simple_loop() {
    let w = compile_or_skip!("unroll", "void main() { float s = 0.0; for (int i = 0; i < 3; ++i) s += uv.x * float(i); o = vec4(s); }");
    let (out, ops, _) = rewrite(&w, "unroll", &Opts::default());
    assert_eq!(count(&ops, "unroll"), 1, "{ops:#?}");
    all_class(&ops, "unroll", Class::Exact);
    assert!(ops[0].detail.contains("3 iterations"), "{}", ops[0].detail);
    let l = Lifted::load(&out).unwrap();
    assert_eq!(l.opcode_histogram().iter().find(|(n, _)| n == "OpLoopMerge").map(|x| x.1).unwrap_or(0), 0);
    check_exact(&w, &out, &[], "unroll");
}

#[test]
fn unroll_gaussian_form_and_cleanup() {
    // The corpus form: i from -4 to 4 inclusive, float(i) feeding math; after fold/dce the
    // induction variable and its loads are gone.
    let w = compile_or_skip!(
        "unroll_g",
        "void main() { float s = 0.0; for (int i = -4; i <= 4; ++i) { float x = float(i); s += x * x * uv.x; } o = vec4(s); }"
    );
    let (out, ops, counts) = rewrite(&w, "fold,dce,cse,ident,unroll", &Opts::default());
    assert_eq!(count(&ops, "unroll"), 1, "{ops:#?}");
    assert!(ops.iter().any(|o| o.pass == "unroll" && o.detail.contains("9 iterations")), "{ops:#?}");
    assert!(ops.iter().any(|o| o.pass == "dce" && o.detail.contains("\"i\" only stored to")), "{ops:#?}");
    assert!(ops.iter().filter(|o| o.pass == "fold" && o.detail.contains("OpConvertSToF")).count() >= 9, "{ops:#?}");
    eprintln!("counts: {counts:?}");
    let l = Lifted::load(&out).unwrap();
    let h = l.opcode_histogram();
    assert!(h.iter().all(|(n, _)| n != "OpLoopMerge" && n != "OpConvertSToF" && n != "OpIAdd"), "{h:?}");
    check_exact(&w, &out, &[], "unroll gaussian");
}

#[test]
fn unroll_nested_loops() {
    let w = compile_or_skip!(
        "unroll_nested",
        "layout(set = 0, binding = 0) uniform sampler2D s;
         float pcf(vec2 p) { float acc = 0.0;
           for (int v = -1; v <= 1; v++) for (int h = -1; h <= 1; h++) acc += texture(s, p + vec2(h, v) * 0.1).r;
           return acc / 9.0; }
         void main() { o = vec4(pcf(uv)); }"
    );
    let (out, ops, _) = rewrite(&w, "unroll", &Opts::default());
    assert_eq!(count(&ops, "unroll"), 2, "{ops:#?}");
    let l = Lifted::load(&out).unwrap();
    let h = l.opcode_histogram();
    let get = |k: &str| h.iter().find(|(n, _)| n == k).map(|x| x.1).unwrap_or(0);
    assert_eq!(get("OpLoopMerge"), 0);
    assert_eq!(get("OpImageSampleImplicitLod"), 9);
    check_exact(&w, &out, &[], "unroll nested");
}

#[test]
fn unroll_respects_max_and_other_shapes() {
    let src = "void main() { float s = 0.0; for (int i = 0; i < 20; ++i) s += uv.x; int k = 5; while (k > 0) { s += 1.0; k -= 2; } o = vec4(s); }";
    let w = compile_or_skip!("unroll_max", src);
    let (_, ops, _) = rewrite(&w, "unroll", &Opts::default());
    // Only the while loop (3 iterations, step -2) fits under 16.
    assert_eq!(count(&ops, "unroll"), 1, "{ops:#?}");
    assert!(ops[0].detail.contains("step -2") && ops[0].detail.contains("3 iterations"), "{}", ops[0].detail);
    let (out, ops, _) = rewrite(&w, "unroll", &Opts { max_unroll: 20, ..Opts::default() });
    assert_eq!(count(&ops, "unroll"), 2, "{ops:#?}");
    check_exact(&w, &out, &[], "unroll max");
    // Non-constant bound: untouched.
    let w2 = compile_or_skip!(
        "unroll_dyn",
        "layout(set = 0, binding = 1) uniform Params { int n; } u; void main() { float s = 0.0; for (int i = 0; i < u.n; ++i) s += uv.x; o = vec4(s); }"
    );
    let (_, ops, _) = rewrite(&w2, "unroll", &Opts::default());
    assert_eq!(count(&ops, "unroll"), 0);
    // A break inside the body: untouched.
    let w3 = compile_or_skip!("unroll_break", "void main() { float s = 0.0; for (int i = 0; i < 4; ++i) { if (uv.x > 0.5) break; s += 1.0; } o = vec4(s); }");
    let (_, ops, _) = rewrite(&w3, "unroll", &Opts::default());
    assert_eq!(count(&ops, "unroll"), 0);
    // A `continue` inside a nested if: untouched (the branch would leave the selection
    // construct once the loop is gone). A plain if inside the body is fine.
    let w4 = compile_or_skip!("unroll_continue", "void main() { float s = 0.0; for (int i = 0; i < 4; ++i) { if (uv.x > 0.5) continue; s += float(i); } o = vec4(s); }");
    let (out4, ops, _) = rewrite(&w4, "unroll,fold,dce", &Opts::default());
    assert_eq!(count(&ops, "unroll"), 0);
    check_exact(&w4, &out4, &[], "unroll continue");
    let w5 = compile_or_skip!("unroll_if", "void main() { float s = 0.0; for (int i = 0; i < 4; ++i) { if (uv.x > 0.5) { s += float(i); } else { s -= 1.0; } } o = vec4(s); }");
    let (out5, ops, _) = rewrite(&w5, "unroll,fold,dce", &Opts::default());
    assert_eq!(count(&ops, "unroll"), 1);
    check_exact(&w5, &out5, &[], "unroll if");
}

// ---- divconst --------------------------------------------------------------------------------

#[test]
fn divconst_ulp_and_only_op() {
    let w = compile_or_skip!(
        "divconst",
        "void main() { vec2 g = uv / 3.0; float h = uv.x / 7.0; float p = uv.y / 4.0; float z = uv.x / 0.0; o = vec4(g, h + p, z); }"
    );
    // glslang builds the splat divisor with OpCompositeConstruct: fold turns it into a constant.
    let (out, ops, _) = rewrite(&w, "fold,divconst", &Opts::default());
    let dc: Vec<&EditOp> = ops.iter().filter(|o| o.pass == "divconst").collect();
    assert_eq!(dc.len(), 3, "{dc:#?}");
    assert_eq!(dc.iter().filter(|o| o.class == Class::Ulp).count(), 2);
    let pow2 = dc.iter().find(|o| o.class == Class::Exact).expect("x / 4.0 is exact");
    assert!(pow2.detail.contains("power of two"), "{}", pow2.detail);
    assert!(dc.iter().any(|o| o.detail.contains("0x3eaaaaab")), "reciprocal bits: {dc:#?}");
    for o in &dc {
        assert_eq!(o.replaced_by, Some(o.target), "in-place rewrite keeps the id");
    }
    let worst = check_ulp(&w, &out, &[], 2, "divconst");
    eprintln!("divconst: worst {worst} ULP");
    // --only-op on the scalar division by 7 (it restricts fold as well, so the vec2 divisor
    // would stay an OpCompositeConstruct).
    let target = dc.iter().find(|o| o.detail.contains("c = 7.0")).unwrap().target;
    let (_, ops, _) = rewrite(&w, "fold,divconst", &Opts { only_op: Some(target), ..Opts::default() });
    assert_eq!(count(&ops, "divconst"), 1);
    assert_eq!(ops.iter().find(|o| o.pass == "divconst").unwrap().target, target);
    // --exact-only keeps only the power-of-two rewrite.
    let (_, ops, _) = rewrite(&w, "fold,divconst", &Opts { exact_only: true, ..Opts::default() });
    assert_eq!(count(&ops, "divconst"), 1);
    assert_eq!(ops.iter().find(|o| o.pass == "divconst").unwrap().class, Class::Exact);
}

// ---- powspec ---------------------------------------------------------------------------------

#[test]
fn powspec_forms() {
    let w = compile_or_skip!(
        "powspec",
        "void main() { float a = pow(uv.x, 2.0); float b = pow(uv.y, 0.5); float c = pow(uv.x, 1.0); float d = pow(uv.y, 3.0);
                       vec2 e = pow(uv, vec2(2.0)); float f = pow(uv.x, 2.5); o = vec4(a + e.x, b + e.y, c + f, d); }"
    );
    let (out, ops, _) = rewrite(&w, "powspec", &Opts::default());
    let ps: Vec<&EditOp> = ops.iter().filter(|o| o.pass == "powspec").collect();
    assert_eq!(ps.len(), 5, "{ps:#?}");
    all_class(&ops, "powspec", Class::Ulp);
    let l = Lifted::load(&out).unwrap();
    let h = l.opcode_histogram();
    let get = |k: &str| h.iter().find(|(n, _)| n == k).map(|x| x.1).unwrap_or(0);
    assert_eq!(get("ExtInst:Pow"), 1, "{h:?}");
    assert_eq!(get("ExtInst:Sqrt"), 1);
    let worst = check_ulp(&w, &out, &[], 2, "powspec");
    eprintln!("powspec: worst {worst} ULP");
    let (_, ops, _) = rewrite(&w, "powspec", &Opts { only_op: Some(ps[0].target), ..Opts::default() });
    assert_eq!(count(&ops, "powspec"), 1);
}

// ---- select ----------------------------------------------------------------------------------

#[test]
fn select_short_circuit_and_negative() {
    let w = compile_or_skip!(
        "select",
        "layout(set = 0, binding = 0) uniform sampler2D s;
         void main() {
           float e = (uv.x < 0.5 || uv.y < 0.5) ? 1.0 : 0.0;
           float f = (uv.x > 0.25 && uv.y > 0.75) ? uv.x : uv.y;
           float g = (uv.x < 0.5 || texture(s, uv).r < 0.5) ? 1.0 : 0.0;   // image op in the arm: not converted
           float h = (uv.x < 0.5 && dFdx(uv.x) > 0.0) ? 1.0 : 0.0;       // derivative in the arm: not converted
           o = vec4(e, f, g, h); }"
    );
    let (out, ops, _) = rewrite(&w, "select", &Opts::default());
    let sel: Vec<&EditOp> = ops.iter().filter(|o| o.pass == "select").collect();
    assert_eq!(sel.len(), 2, "{sel:#?}");
    all_class(&ops, "select", Class::Exact);
    let l = Lifted::load(&out).unwrap();
    let h = l.opcode_histogram();
    let get = |k: &str| h.iter().find(|(n, _)| n == k).map(|x| x.1).unwrap_or(0);
    // The two negative cases keep their phis (the `? uv.x : uv.y` ternary is a store-based
    // if/else in glslang's output and is not a candidate either).
    assert_eq!(get("OpPhi"), 2, "{h:?}");
    assert!(get("OpSelectionMerge") >= 2);
    check_exact(&w, &out, &[], "select");
    // --only-op by phi id.
    let phi = sel[0].detail.split("OpSelect (%").nth(1).unwrap().split(')').next().unwrap().parse::<u32>().unwrap();
    let (_, ops, _) = rewrite(&w, "select", &Opts { only_op: Some(phi), ..Opts::default() });
    assert_eq!(count(&ops, "select"), 1);
}

#[test]
fn select_two_arms_with_phi() {
    // spirv-opt-style SSA if/else: build it from a ternary whose arms are pure vector math;
    // glslang emits OpSelect for trivial arms but branches + phi for these.
    let w = compile_or_skip!(
        "select2",
        "void main() { vec3 c = (uv.x < 0.5) ? (uv.xyx * 2.0 + 1.0) : (uv.yxy * 3.0 - 1.0); o = vec4(c, 1.0); }"
    );
    let l = Lifted::load(&w).unwrap();
    let phis = l.opcode_histogram().iter().find(|(n, _)| n == "OpPhi").map(|x| x.1).unwrap_or(0);
    let (out, ops, _) = rewrite(&w, "select", &Opts::default());
    if phis == 0 {
        eprintln!("glslang emitted no phi for the ternary; nothing to convert");
        assert_eq!(count(&ops, "select"), 0);
        return;
    }
    assert_eq!(count(&ops, "select"), 1, "{ops:#?}");
    check_exact(&w, &out, &[], "select two arms");
}

// ---- ops.json format -------------------------------------------------------------------------

#[test]
fn ops_json_format() {
    let op = EditOp { pass: "fold", class: Class::Exact, target: 57, replaced_by: Some(401), detail: "OpFMul %55 %56 -> OpConstant 0.5".into() };
    let j = op.to_json();
    assert_eq!(j["pass"], "fold");
    assert_eq!(j["class"], "exact");
    assert_eq!(j["target"], 57);
    assert_eq!(j["replaced_by"], 401);
    assert_eq!(j["detail"], "OpFMul %55 %56 -> OpConstant 0.5");
    let none = EditOp { pass: "dce", class: Class::Exact, target: 1, replaced_by: None, detail: String::new() };
    assert!(none.to_json()["replaced_by"].is_null());
}

// ---- corpus ----------------------------------------------------------------------------------

fn leaf_count(l: &Lifted, ty: u32) -> usize {
    match l.ty(ty).unwrap() {
        Type::Vector { count, .. } => *count as usize,
        Type::Matrix { column, columns } => *columns as usize * leaf_count(l, *column),
        Type::Array { elem, len } => l.array_len(*len).unwrap() as usize * leaf_count(l, *elem),
        Type::Struct { members } => members.iter().map(|m| leaf_count(l, *m)).sum(),
        _ => 1,
    }
}

/// Generic inputs from reflection: a gradient image for every sampler, ones for every uniform
/// member (zero for `pad*`).
fn corpus_run(l: &Lifted, w: usize, h: usize, mode: Mode) -> Run {
    let mut r = Run::new(w, h, mode);
    for v in &l.variables {
        match v.storage {
            StorageClass::UniformConstant => {
                let name = v.name.clone().unwrap_or_else(|| format!("binding{}", v.binding.unwrap_or(0)));
                r = r.sampler(&name, gradient(32, 32), Filter::Linear);
            }
            StorageClass::Uniform | StorageClass::PushConstant => {
                if let Type::Struct { members } = l.ty(v.pointee).unwrap() {
                    for (m, mty) in members.iter().enumerate() {
                        let name = l.member_name(v.pointee, m as u32).map(String::from).unwrap_or_else(|| format!("member{m}"));
                        let fill = if name.starts_with("pad") { "0" } else { "1" };
                        let n = leaf_count(l, *mty);
                        let text = format!("[{}]", vec![fill; n].join(","));
                        r = r.uniform(&name, &text);
                    }
                }
            }
            _ => {}
        }
    }
    r
}

fn corpus_modules() -> Vec<std::path::PathBuf> {
    let dir = Path::new(SPV_DIR);
    if !dir.is_dir() {
        eprintln!("SKIP: no corpus at {SPV_DIR}");
        return vec![];
    }
    let mut v: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map_or(false, |e| e == "spv"))
        .collect();
    v.sort();
    v
}

#[test]
fn corpus_full_pipeline_validates() {
    for p in corpus_modules() {
        let words = shader_ir::read_spv(&p).unwrap();
        let (out, ops, counts) = rewrite(&words, "fold,dce,cse,ident,unroll,divconst,powspec,select", &Opts::default());
        let l0 = Lifted::load(&words).unwrap();
        let l1 = Lifted::load(&out).unwrap();
        eprintln!(
            "{}: {} -> {} instructions, {} ops, counts {:?}",
            p.file_name().unwrap().to_string_lossy(),
            passes::instruction_count(&l0),
            passes::instruction_count(&l1),
            ops.len(),
            counts
        );
        // Untouched ids keep their meaning: every id below the old bound that still exists is
        // defined by an instruction with the same opcode.
        for (id, site) in &l1.defs {
            if *id >= l0.bound {
                continue;
            }
            let (Some(a), Some(b)) = (
                l0.defs.get(id).and_then(|s| passes::inst_at(&l0, *s)),
                passes::inst_at(&l1, *site),
            ) else { continue };
            let rewritten = ops.iter().any(|o| o.target == *id && o.replaced_by == Some(*id));
            let phi_to_select = a.class.opcode == spirv::Op::Phi && b.class.opcode == spirv::Op::Select;
            assert!(a.class.opcode == b.class.opcode || rewritten || phi_to_select, "%{id}: {:?} became {:?}", a.class.opcode, b.class.opcode);
        }
    }
}

#[test]
fn corpus_exact_pipeline_is_bit_identical_in_f64() {
    for p in corpus_modules() {
        let words = shader_ir::read_spv(&p).unwrap();
        let (out, ops, _) = rewrite(&words, "fold,dce,cse,ident,unroll,select", &Opts { exact_only: true, ..Opts::default() });
        all_class(&ops, "fold", Class::Exact);
        let l0 = Lifted::load(&words).unwrap();
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        let a = corpus_run(&l0, 64, 36, Mode::F64).eval_ok(&words);
        let b = corpus_run(&l0, 64, 36, Mode::F64).eval_ok(&out);
        assert_bit_identical(&a, &b, &name);
        eprintln!("{name}: {} exact ops, bit-identical on 64x36 (discarded {})", ops.len(), a.discarded_pixels);
    }
}

#[test]
fn corpus_full_pipeline_within_ulp_in_f32() {
    for p in corpus_modules() {
        let words = shader_ir::read_spv(&p).unwrap();
        let (out, _, _) = rewrite(&words, "fold,dce,cse,ident,unroll,divconst,powspec,select", &Opts::default());
        let l0 = Lifted::load(&words).unwrap();
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        let a = corpus_run(&l0, 64, 36, Mode::F32).eval_ok(&words);
        let b = corpus_run(&l0, 64, 36, Mode::F32).eval_ok(&out);
        let worst = assert_within_ulp(&a, &b, 2, &name);
        eprintln!("{name}: worst {worst} ULP in f32 on the full pipeline");
    }
}
