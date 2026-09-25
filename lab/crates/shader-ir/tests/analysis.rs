//! M2 analysis tests: rates, sinks, sampler coordinate kinds, ranges, f16 sites, line mapping.

mod common;

use common::{px, tmp_dir, Run, GLSLANG, PRELUDE};
use shader_ir::analysis::{self, AnalyzeOptions, Analysis, CoordKind, Entry, Rate, Sink};
use shader_ir::interp::{Filter, Mode};
use shader_ir::lift::Lifted;
use shader_ir::npy::Image;
use std::path::Path;
use std::process::Command;

/// Compiles with the pinned glslang; `debug` adds `-g`. None when glslang is absent.
fn compile(name: &str, src: &str, debug: bool) -> Option<Vec<u32>> {
    let frag = tmp_dir().join(format!("{name}.frag"));
    std::fs::write(&frag, format!("{PRELUDE}{src}")).unwrap();
    compile_path(&frag, debug)
}

fn compile_path(frag: &Path, debug: bool) -> Option<Vec<u32>> {
    if !common::glslang_available() {
        return None;
    }
    let name = frag.file_stem().unwrap().to_string_lossy().to_string();
    let out = tmp_dir().join(format!("{name}{}.spv", if debug { ".g" } else { "" }));
    let mut cmd = Command::new(GLSLANG);
    cmd.arg("-V");
    if debug {
        cmd.arg("-g");
    }
    let st = cmd.arg("-o").arg(&out).arg(frag).output().expect("run glslang");
    assert!(st.status.success(), "glslang failed:\n{}{}", String::from_utf8_lossy(&st.stdout), String::from_utf8_lossy(&st.stderr));
    Some(shader_ir::read_spv(&out).unwrap())
}

fn analyze(words: &[u32], debug: Option<&[u32]>) -> (Lifted, Analysis) {
    let lifted = Lifted::load(words).unwrap();
    let dbg = debug.map(|w| Lifted::load(w).unwrap());
    let a = analysis::analyze(&lifted, &AnalyzeOptions { shader: "t".into(), debug: dbg.as_ref(), ranges: None }).unwrap();
    (lifted, a)
}

fn var_id(l: &Lifted, name: &str) -> u32 {
    *l.names.iter().find(|(_, n)| n.as_str() == name).unwrap_or_else(|| panic!("no OpName {name}")).0
}

fn entry(a: &Analysis, id: u32) -> &Entry {
    a.instructions.iter().find(|e| e.id == id).unwrap_or_else(|| panic!("no entry %{id}"))
}

/// Entries of `op` whose first operand is `id`.
fn using<'a>(a: &'a Analysis, op: &str, id: u32) -> Vec<&'a Entry> {
    a.instructions.iter().filter(|e| e.op == op && e.operands.first() == Some(&id)).collect()
}

fn stores_to<'a>(a: &'a Analysis, var: u32) -> Vec<&'a Entry> {
    using(a, "OpStore", var)
}

const RATES_SRC: &str = "layout(set = 0, binding = 1) uniform Params { float sigma; vec2 texel; } u;
layout(set = 0, binding = 0) uniform sampler2D t;
void main() {
  float wsum = 0.0;
  vec3 acc = vec3(0.0);
  for (int i = -4; i <= 4; ++i) {
    float x = float(i);
    float w = exp(-0.5 * x * x / (u.sigma * u.sigma));
    acc += texture(t, uv + vec2(x * u.texel.x, 0.0)).rgb * w;
    wsum += w;
  }
  float k = uv.x * 2.0;
  o = vec4(acc / wsum, k);
}
";

#[test]
fn rates_const_loop_uniform_subexpression_pixel_math() {
    let Some(w) = compile("rates", RATES_SRC, false) else { return };
    let (l, a) = analyze(&w, None);
    // Loop counter: every load of `i` and every store to it is const.
    let i = var_id(&l, "i");
    let loads = using(&a, "OpLoad", i);
    assert!(loads.len() >= 3, "loads of i: {}", loads.len());
    for e in &loads {
        assert_eq!(e.rate, Rate::Const, "load of i %{}", e.id);
    }
    for e in stores_to(&a, i) {
        assert_eq!(e.rate, Rate::Const, "store to i");
    }
    // x = float(i) is const; w = exp(... sigma ...) is uniform; the Exp itself is uniform.
    assert!(stores_to(&a, var_id(&l, "x")).iter().all(|e| e.rate == Rate::Const));
    assert!(stores_to(&a, var_id(&l, "w")).iter().all(|e| e.rate == Rate::Uniform));
    let exp = a.instructions.iter().find(|e| e.ext.as_deref() == Some("Exp")).expect("Exp");
    assert_eq!(exp.rate, Rate::Uniform);
    assert_eq!(exp.op, "OpExtInst");
    assert_eq!(exp.ty.as_deref(), Some("f32"));
    // wsum accumulates uniform values; acc accumulates texture reads.
    assert!(using(&a, "OpLoad", var_id(&l, "wsum")).iter().all(|e| e.rate == Rate::Uniform));
    assert!(using(&a, "OpLoad", var_id(&l, "acc")).iter().all(|e| e.rate == Rate::Pixel));
    // k = uv.x * 2.0 is pixel; the sample and the uv load are pixel; the uniform loads uniform.
    assert!(stores_to(&a, var_id(&l, "k")).iter().all(|e| e.rate == Rate::Pixel));
    assert!(a.instructions.iter().filter(|e| e.op == "OpImageSampleImplicitLod").all(|e| e.rate == Rate::Pixel));
    assert_eq!(using(&a, "OpLoad", var_id(&l, "uv"))[0].rate, Rate::Pixel);
    let uni_loads: Vec<&Entry> = a
        .instructions
        .iter()
        .filter(|e| {
            e.op == "OpLoad"
                && e.ty.as_deref() == Some("f32")
                && e.operands.first().map_or(false, |&p| entry(&a, p).op == "OpAccessChain" && entry(&a, p).operands[0] == var_id(&l, "u"))
        })
        .collect();
    assert!(!uni_loads.is_empty());
    assert!(uni_loads.iter().all(|e| e.rate == Rate::Uniform), "{:?}", uni_loads.iter().map(|e| (e.id, e.rate)).collect::<Vec<_>>());
    // Every result-bearing entry has a type spelled the M2 way; labels have none.
    assert!(a.instructions.iter().filter(|e| e.op == "OpLabel").all(|e| e.ty.is_none() && e.id != 0));
    assert!(a.instructions.iter().any(|e| e.ty.as_deref() == Some("vec3<f32>")));
    assert!(a.instructions.iter().any(|e| e.ty.as_deref() == Some("ptr(Function, i32)")));
    assert!(a.instructions.iter().any(|e| e.ty.as_deref() == Some("bool")));
    assert_eq!(a.summary.pixel + a.summary.uniform + a.summary.const_, a.instructions.iter().filter(|e| e.is_site).count());
    assert!(a.summary.const_ >= 3 && a.summary.uniform >= 5 && a.summary.pixel >= 5, "{:?}", a.summary);
    assert_eq!(a.outputs.len(), 1);
    assert_eq!(a.outputs[0].ty, "vec4<f32>");
    assert_eq!(a.functions[0].call_sites, 0);
}

const SINKS_SRC: &str = "layout(set = 0, binding = 1) uniform Params { vec2 off; float thr; } u;
layout(set = 0, binding = 0) uniform sampler2D t;
void main() {
  vec4 c = texture(t, uv + u.off);
  if (c.r > u.thr) { o = c; } else { o = vec4(0.0); }
  if (c.g > 0.9) discard;
  int n = int(c.b * 4.0);
  o.a = float(n) + u.thr;
}
";

#[test]
fn sinks_address_control_discard_convert() {
    let Some(w) = compile("sinks", SINKS_SRC, false) else { return };
    let (l, a) = analyze(&w, None);
    let has = |e: &Entry, s: Sink| e.sinks.contains(&s);
    // uv + off: the add, the uv load and the uniform load of `off` are all address sinks.
    let add = a.instructions.iter().find(|e| e.op == "OpFAdd" && e.ty.as_deref() == Some("vec2<f32>")).expect("uv + off");
    assert!(has(add, Sink::Address), "{:?}", add.sinks);
    assert_eq!(add.rate, Rate::Pixel);
    for &o in &add.operands {
        let e = entry(&a, o);
        assert_eq!(e.op, "OpLoad");
        assert!(has(e, Sink::Address), "%{o} {:?}", e.sinks);
    }
    let uv_load = using(&a, "OpLoad", var_id(&l, "uv"))[0];
    assert!(has(uv_load, Sink::Address));
    let off_load = add.operands.iter().map(|&o| entry(&a, o)).find(|e| e.rate == Rate::Uniform).expect("uniform load of off");
    assert_eq!(off_load.sinks, vec![Sink::Address]);
    // The f32 uniform loads of `thr`: one feeds the branch (control only), one the output (none).
    let thr_loads: Vec<&Entry> = a.instructions.iter().filter(|e| e.op == "OpLoad" && e.ty.as_deref() == Some("f32") && e.rate == Rate::Uniform).collect();
    assert_eq!(thr_loads.len(), 2, "{:?}", thr_loads.iter().map(|e| e.id).collect::<Vec<_>>());
    assert_eq!(thr_loads[0].sinks, vec![Sink::Control]);
    assert!(thr_loads[1].sinks.is_empty());
    assert!(!thr_loads.iter().any(|e| has(e, Sink::Address)));
    // Comparisons: the first is control only, the second (discard) is control + discard.
    let cmps: Vec<&Entry> = a.instructions.iter().filter(|e| e.op == "OpFOrdGreaterThan").collect();
    assert_eq!(cmps.len(), 2);
    assert!(has(cmps[0], Sink::Control) && !has(cmps[0], Sink::Discard), "{:?}", cmps[0].sinks);
    assert!(has(cmps[1], Sink::Control) && has(cmps[1], Sink::Discard), "{:?}", cmps[1].sinks);
    // c.b * 4.0 feeds int(): convert.
    let mul = a.instructions.iter().find(|e| e.op == "OpFMul").expect("c.b * 4.0");
    assert_eq!(mul.sinks, vec![Sink::Convert]);
    let cvt = a.instructions.iter().find(|e| e.op == "OpConvertFToS").unwrap();
    assert_eq!(cvt.operands, vec![mul.id]);
    // The texture read itself feeds every sink through the `c` variable, but is not an address.
    let sample = a.instructions.iter().find(|e| e.op == "OpImageSampleImplicitLod").unwrap();
    assert!(has(sample, Sink::Control) && has(sample, Sink::Discard) && has(sample, Sink::Convert), "{:?}", sample.sinks);
    assert!(!has(sample, Sink::Address));
    // The final add `float(n) + u.thr` feeds only the output: a candidate site.
    let out_add = a.instructions.iter().filter(|e| e.op == "OpFAdd" && e.ty.as_deref() == Some("f32")).last().unwrap();
    assert!(out_add.sinks.is_empty());
    assert!(a.summary.sink_sites >= 6 && a.summary.candidate_sites >= 1, "{:?}", a.summary);
    assert_eq!(a.summary.float_sites, a.instructions.iter().filter(|e| e.is_float).count());
}

#[test]
fn sinks_through_function_parameters_and_select() {
    let Some(w) = compile(
        "sinks_fn",
        "layout(set = 0, binding = 1) uniform Params { float thr; } u;
         float luma(vec3 c) { return dot(c, vec3(0.3, 0.6, 0.1)); }
         void main() {
           vec3 a = vec3(uv, 0.5);
           float l = luma(a * 2.0);
           float s = (l > u.thr) ? 1.0 : 0.0;
           o = vec4(s, l, 0.0, 1.0);
         }",
        false,
    ) else {
        return;
    };
    let (_l, a) = analyze(&w, None);
    let dot = a.instructions.iter().find(|e| e.op == "OpDot").unwrap();
    assert!(dot.sinks.contains(&Sink::Control), "{:?}", dot.sinks);
    assert_eq!(dot.func, "luma(vf3;");
    let param = a.instructions.iter().find(|e| e.op == "OpFunctionParameter").unwrap();
    assert!(param.sinks.contains(&Sink::Control));
    assert!(param.block.is_none());
    assert_eq!(param.rate, Rate::Pixel);
    // a * 2.0 in the caller feeds the parameter.
    let mul = a.instructions.iter().find(|e| e.op == "OpVectorTimesScalar").unwrap();
    assert!(mul.sinks.contains(&Sink::Control), "{:?}", mul.sinks);
    let sel = a.instructions.iter().find(|e| e.op == "OpSelect").unwrap();
    assert!(sel.sinks.is_empty());
    assert!(entry(&a, sel.operands[0]).sinks.contains(&Sink::Control));
    assert_eq!(a.functions.iter().find(|f| f.name == "luma(vf3;").unwrap().call_sites, 1);
}

#[test]
fn coord_kinds() {
    let Some(w) = compile(
        "coord",
        "layout(set = 0, binding = 1) uniform Params { vec2 texel; } u;
         layout(set = 0, binding = 0) uniform sampler2D t;
         void main() {
           vec4 a = texture(t, uv);
           vec4 b = texture(t, uv + vec2(0.001, 0.0));
           vec4 c = texture(t, uv + u.texel * vec2(1, 0));
           vec4 d = texture(t, uv * 2.0);
           vec4 e = texture(t, uv - vec2(0.25));
           vec2 p = uv;
           vec4 f = texture(t, p);
           vec4 g = texture(t, uv + vec2(uv.x, 0.0));
           o = a + b + c + d + e + f + g;
         }",
        false,
    ) else {
        return;
    };
    let (_l, a) = analyze(&w, None);
    assert_eq!(a.samplers.len(), 1);
    let s = &a.samplers[0];
    assert_eq!(s.name, "t");
    assert_eq!(s.binding, Some(0));
    let kinds: Vec<(CoordKind, Option<[f64; 2]>)> = s.samples.iter().map(|x| (x.coord_kind, x.offset)).collect();
    assert_eq!(kinds.len(), 7, "{kinds:?}");
    assert_eq!(kinds[0], (CoordKind::UvExact, None));
    assert_eq!(kinds[1].0, CoordKind::UvOffset);
    let off = kinds[1].1.unwrap();
    assert!((off[0] - 0.001).abs() < 1e-9 && off[1] == 0.0, "{off:?}");
    assert_eq!(kinds[2], (CoordKind::UvOffset, None));
    assert_eq!(kinds[3], (CoordKind::Other, None));
    assert_eq!(kinds[4], (CoordKind::UvOffset, Some([-0.25, -0.25])));
    assert_eq!(kinds[5], (CoordKind::UvExact, None));
    assert_eq!(kinds[6], (CoordKind::Other, None));
    for x in &s.samples {
        let e = entry(&a, x.id);
        assert!(e.op.starts_with("OpImageSample"));
        assert_eq!(e.operands[1], x.coord_id);
        assert!(entry(&a, x.coord_id).sinks.contains(&Sink::Address));
    }
}

fn gradient(n: usize) -> Image {
    let mut img = Image::new(n, n);
    for y in 0..n {
        for x in 0..n {
            img.set_texel(x, y, [x as f32 / n as f32, y as f32 / n as f32, 0.5, 1.0]);
        }
    }
    img
}

#[test]
fn ranges_on_gradient_sampler() {
    let Some(w) = compile(
        "ranges",
        "layout(set = 0, binding = 0) uniform sampler2D t;
         void main() { vec4 c = texture(t, uv) * 2.0; o = vec4(c.rgb, 1.0 / c.r); }",
        false,
    ) else {
        return;
    };
    let (l, a) = analyze(&w, None);
    let scaled = a.instructions.iter().find(|e| e.op == "OpVectorTimesScalar").unwrap().id;
    let div = a.instructions.iter().find(|e| e.op == "OpFDiv").unwrap().id;
    let mut run = Run::new(8, 8, Mode::F32).sampler("t", gradient(8), Filter::Nearest);
    run.cfg.profile = true;
    let out = run.eval_ok(&w);
    let r = out.ranges.as_ref().expect("ranges");
    let s = &r[&scaled];
    assert_eq!(s.samples, 64);
    assert_eq!((s.min, s.max, s.nan, s.inf), (0.0, 2.0, 0, 0)); // 2 * [0, 7/8] rgb, 2 * 1 alpha
    let d = &r[&div];
    assert_eq!(d.samples, 64);
    assert_eq!(d.inf, 8, "1/0 on the left column"); // x = 0 column
    assert_eq!(d.nan, 0);
    assert!((d.min - 1.0 / 1.75).abs() < 1e-6 && (d.max - 1.0 / 0.25).abs() < 1e-6, "{d:?}");
    // Non-float results are not profiled; the uv load is.
    assert!(!r.contains_key(&var_id(&l, "t")));
    assert!(r.contains_key(&using(&a, "OpLoad", var_id(&l, "uv"))[0].id));
    // Stride 2: quads at (0,0), (4,0), (0,4), (4,4) -> 16 pixels; others are not evaluated.
    let mut run = Run::new(8, 8, Mode::F32).sampler("t", gradient(8), Filter::Nearest);
    run.cfg.profile = true;
    run.cfg.stride = 2;
    let out2 = run.eval_ok(&w);
    let r2 = out2.ranges.as_ref().unwrap();
    assert_eq!(r2[&scaled].samples, 16);
    assert_eq!(r2[&div].inf, 4);
    assert_eq!(px(&out2, 0, 0)[0], 0.0);
    assert_eq!(px(&out2, 5, 5)[0], 2.0 * 5.0 / 8.0);
    assert!(px(&out2, 2, 0)[0].is_nan(), "pixel outside the stride is not evaluated");
    // The ranges attach to the analysis and survive the JSON round trip.
    let text = serde_json::to_string(&analysis::ranges_json(r)).unwrap();
    let parsed = analysis::parse_ranges(&text).unwrap();
    assert_eq!(parsed[&scaled], *s);
    let with = analysis::analyze(&l, &AnalyzeOptions { shader: "t".into(), debug: None, ranges: Some(&parsed) }).unwrap();
    assert_eq!(entry(&with, scaled).range.unwrap().max, 2.0);
    let sampler_load = with.instructions.iter().find(|e| e.ty.as_deref() == Some("sampler2D")).unwrap();
    assert!(sampler_load.range.is_none(), "non-float results have no range");
    let j = with.to_json();
    assert_eq!(j["instructions"].as_array().unwrap().iter().find(|e| e["id"] == scaled).unwrap()["range"]["samples"], 64);
}

#[test]
fn f16_sites_round_a_known_value() {
    let Some(w) = compile(
        "f16sites",
        "layout(set = 0, binding = 1) uniform Params { float v; float s; } u;
         void main() { float a = u.v * u.s; o = vec4(a, a * 2.0, u.v, 1.0); }",
        false,
    ) else {
        return;
    };
    let (_l, a) = analyze(&w, None);
    let muls: Vec<u32> = a.instructions.iter().filter(|e| e.op == "OpFMul").map(|e| e.id).collect();
    assert_eq!(muls.len(), 2);
    let v = 1.0 + 2f32.powi(-12); // representable in f32, rounds to 1 in f16
    let vs = format!("{v}");
    let plain = Run::new(2, 2, Mode::F32).uniform("v", &vs).uniform("s", "1.0").eval_ok(&w);
    assert_eq!(px(&plain, 0, 0), [v, 2.0 * v, v, 1.0]);
    let mut run = Run::new(2, 2, Mode::F32).uniform("v", &vs).uniform("s", "1.0");
    run.cfg.f16_sites = vec![muls[0]];
    let rounded = run.eval_ok(&w);
    assert_eq!(px(&rounded, 1, 1), [1.0, 2.0, v, 1.0], "only the listed site is rounded");
    let mut run = Run::new(2, 2, Mode::F32).uniform("v", &vs).uniform("s", "1.0");
    run.cfg.f16_all = true;
    let all = run.eval_ok(&w);
    // u.v itself is a load (rounded too under --f16-all: every float-typed result).
    assert_eq!(px(&all, 0, 1), [1.0, 2.0, 1.0, 1.0]);
    // A value that f16 cannot hold overflows to inf at the listed site only.
    let big = Run::new(1, 1, Mode::F32).uniform("v", "70000.0").uniform("s", "1.0").eval_ok(&w);
    assert_eq!(px(&big, 0, 0)[0], 70000.0);
    let mut run = Run::new(1, 1, Mode::F32).uniform("v", "70000.0").uniform("s", "1.0");
    run.cfg.f16_sites = vec![muls[0]];
    let big16 = run.eval_ok(&w);
    assert!(px(&big16, 0, 0)[0].is_infinite() && px(&big16, 0, 0)[2] == 70000.0);
    // Listing a non-float id is an error.
    let mut run = Run::new(1, 1, Mode::F32).uniform("v", "1.0").uniform("s", "1.0");
    run.cfg.f16_sites = vec![a.instructions.iter().find(|e| e.op == "OpLabel").unwrap().id];
    let err = run.eval(&w).unwrap_err().to_string();
    assert!(err.contains("not a float-typed result id"), "{err}");
}

#[test]
fn line_mapping_against_debug_build() {
    let Some(w) = compile("lines", RATES_SRC, false) else { return };
    let g = compile("lines", RATES_SRC, true).unwrap();
    assert_ne!(w, g);
    let (l, a) = analyze(&w, Some(&g));
    let src_line = |needle: &str| -> u32 {
        let full = format!("{PRELUDE}{RATES_SRC}");
        full.lines().position(|s| s.contains(needle)).map(|p| p as u32 + 1).unwrap()
    };
    let exp = a.instructions.iter().find(|e| e.ext.as_deref() == Some("Exp")).unwrap();
    assert_eq!(exp.line, Some(src_line("exp(")));
    let sample = a.instructions.iter().find(|e| e.op == "OpImageSampleImplicitLod").unwrap();
    assert_eq!(sample.line, Some(src_line("texture(")));
    assert!(stores_to(&a, var_id(&l, "k")).iter().all(|e| e.line == Some(src_line("float k"))));
    assert!(a.instructions.iter().find(|e| e.op == "OpReturn").unwrap().line.is_some());
    // Ids are the measured build's, not the debug build's: the OpName table of `w` agrees.
    assert_eq!(entry(&a, var_id(&l, "acc")).name.as_deref(), Some("acc"));
    // Every entry has an index equal to its position in its function.
    for (k, e) in a.instructions.iter().enumerate() {
        assert_eq!(e.index, k, "single-function module: index == position");
    }
    // A debug build of a different shader is rejected.
    let other = compile("lines_other", SINKS_SRC, true).unwrap();
    let dbg = Lifted::load(&other).unwrap();
    let err = analysis::analyze(&l, &AnalyzeOptions { shader: "t".into(), debug: Some(&dbg), ranges: None }).unwrap_err().to_string();
    assert!(err.contains("debug build"), "{err}");
}

#[test]
fn cli_analyze_and_profile() {
    let Some(w) = compile("cli", RATES_SRC, false) else { return };
    let g = compile("cli", RATES_SRC, true).unwrap();
    let d = tmp_dir();
    let spv = d.join("cli.spv");
    let gspv = d.join("cli.g.spv");
    std::fs::write(&spv, shader_ir::bytes_from_words(&w)).unwrap();
    std::fs::write(&gspv, shader_ir::bytes_from_words(&g)).unwrap();
    let npy = d.join("grad.npy");
    shader_ir::npy::write(&npy, &gradient(8)).unwrap();
    let bin = env!("CARGO_BIN_EXE_shader-ir");
    let ranges = d.join("ranges.json");
    let out = Command::new(bin)
        .args(["eval", "--spv"])
        .arg(&spv)
        .args(["--width", "8", "--height", "8", "--uniform", "sigma=2.0", "--uniform", "texel=[0.125,0.125]", "--sampler"])
        .arg(format!("t={}", npy.display()))
        .arg("--out")
        .arg(d.join("o.npy"))
        .arg("--profile")
        .arg(&ranges)
        .args(["--stride", "2", "--f16-sites", "0"])
        .output()
        .unwrap();
    assert!(!out.status.success(), "id 0 is not a float site");
    let out = Command::new(bin)
        .args(["eval", "--spv"])
        .arg(&spv)
        .args(["--width", "8", "--height", "8", "--uniform", "sigma=2.0", "--uniform", "texel=[0.125,0.125]", "--sampler"])
        .arg(format!("t={}", npy.display()))
        .arg("--out")
        .arg(d.join("o.npy"))
        .arg("--profile")
        .arg(&ranges)
        .args(["--stride", "2", "--f16-all"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let r: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&ranges).unwrap()).unwrap();
    assert!(r.as_object().unwrap().values().all(|v| v["samples"].as_u64().is_some() && v.get("min").is_some() && v.get("nan").is_some()));
    let json = d.join("analysis.json");
    let out = Command::new(bin)
        .args(["analyze", "--spv"])
        .arg(&spv)
        .arg("--debug-spv")
        .arg(&gspv)
        .arg("--ranges")
        .arg(&ranges)
        .arg("--out")
        .arg(&json)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&json).unwrap()).unwrap();
    assert_eq!(j["shader"], "cli");
    assert_eq!(j["entry"], "main");
    assert!(j["bound"].as_u64().unwrap() > 0);
    for key in ["instructions", "samplers", "outputs", "summary"] {
        assert!(j.get(key).is_some(), "{key}");
    }
    let inst = j["instructions"].as_array().unwrap();
    for key in ["id", "op", "ext", "type", "func", "block", "index", "line", "name", "rate", "sinks", "operands", "range"] {
        assert!(inst[0].get(key).is_some(), "{key}");
    }
    let exp = inst.iter().find(|e| e["ext"] == "Exp").unwrap();
    assert_eq!(exp["rate"], "uniform");
    assert!(exp["line"].as_u64().is_some());
    assert!(exp["range"]["samples"].as_u64().unwrap() > 0);
    assert_eq!(j["samplers"][0]["samples"][0]["coord_kind"], "uv_offset");
    assert_eq!(j["outputs"][0]["type"], "vec4<f32>");
    for key in ["pixel", "uniform", "const", "sink_sites", "float_sites", "candidate_sites"] {
        assert!(j["summary"][key].as_u64().is_some(), "{key}");
    }
}

#[test]
fn corpus_analyzes_with_debug_builds() {
    if !common::glslang_available() {
        return;
    }
    let dir = Path::new(common::CORPUS_DIR);
    let mut n = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let p = entry.unwrap().path();
        if p.extension().map_or(true, |e| e != "frag") {
            continue;
        }
        let name = p.file_stem().unwrap().to_string_lossy().to_string();
        let w = compile_path(&p, false).unwrap();
        let g = compile_path(&p, true).unwrap();
        let (_l, a) = analyze(&w, Some(&g));
        assert!(a.instructions.iter().filter(|e| e.line.is_some()).count() > a.instructions.len() / 2, "{name}: lines");
        assert!(a.summary.float_sites > 0 && !a.samplers.is_empty(), "{name}");
        n += 1;
    }
    assert!(n >= 9, "{n} corpus shaders");
}
