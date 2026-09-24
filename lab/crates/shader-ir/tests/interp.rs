//! Interpreter tests on tiny GLSL fragments compiled with the pinned glslang.

mod common;

use common::{assert_close, compile_src, px, Run, PRELUDE};
use shader_ir::interp::{Filter, Mode};
use shader_ir::npy::Image;
use std::path::Path;

macro_rules! compile_or_skip {
    ($name:expr, $src:expr) => {
        match compile_src($name, &format!("{PRELUDE}{}", $src), false) {
            Some(w) => w,
            None => return,
        }
    };
}

#[test]
fn constant_color() {
    let w = compile_or_skip!("constant", "void main() { o = vec4(0.25, 0.5, 0.75, 1.0); }");
    let out = Run::new(5, 3, Mode::F32).eval_ok(&w);
    assert_eq!(out.discarded_pixels, 0);
    for y in 0..3 {
        for x in 0..5 {
            assert_eq!(px(&out, x, y), [0.25, 0.5, 0.75, 1.0]);
        }
    }
}

#[test]
fn uv_passthrough_and_orientation() {
    let w = compile_or_skip!("uv", "void main() { o = vec4(uv, gl_FragCoord.xy); }");
    let (wd, ht) = (8usize, 4usize);
    let out = Run::new(wd, ht, Mode::F32).eval_ok(&w);
    assert_eq!(px(&out, 0, 0), [0.5 / 8.0, 0.5 / 4.0, 0.5, 0.5]);
    assert_eq!(px(&out, 7, 3), [7.5 / 8.0, 3.5 / 4.0, 7.5, 3.5]);
    // Row 0 is the top: v grows downwards, u grows to the right.
    assert!(px(&out, 0, 1)[1] > px(&out, 0, 0)[1]);
    assert!(px(&out, 1, 0)[0] > px(&out, 0, 0)[0]);
    // f64 mode widens the same inputs.
    let out64 = Run::new(wd, ht, Mode::F64).eval_ok(&w);
    assert_eq!(px(&out64, 3, 2), [3.5 / 8.0, 2.5 / 4.0, 3.5, 2.5]);
}

#[test]
fn loop_sum() {
    let w = compile_or_skip!(
        "loop",
        "layout(set = 0, binding = 0) uniform Params { int n; float step; } u;
         void main() {
           float s = 0.0; int k = 0;
           for (int i = 0; i < u.n; ++i) { s += u.step * float(i); }
           while (k < 3) { k++; if (k == 2) continue; s += 100.0; }
           do { s += 1000.0; } while (false);
           o = vec4(s, float(k), 0.0, 1.0);
         }"
    );
    let out = Run::new(2, 2, Mode::F32).uniform("n", "10").uniform("step", "0.5").eval_ok(&w);
    // 0.5 * (0+1+...+9) = 22.5; two while iterations add 100 each; do-while adds 1000.
    assert_eq!(px(&out, 1, 1), [1222.5, 3.0, 0.0, 1.0]);
}

#[test]
fn if_else_with_discard() {
    let w = compile_or_skip!(
        "discard",
        "void main() {
           if (uv.x < 0.5) { discard; } else { o = vec4(1.0, 2.0, 3.0, 4.0); }
         }"
    );
    let out = Run::new(4, 2, Mode::F32).eval_ok(&w);
    assert_eq!(out.discarded_pixels, 4);
    for y in 0..2 {
        for x in 0..2 {
            assert!(px(&out, x, y).iter().all(|c| c.is_nan()), "pixel {x},{y} should be discarded");
        }
        for x in 2..4 {
            assert_eq!(px(&out, x, y), [1.0, 2.0, 3.0, 4.0]);
        }
    }
    // A custom discard value.
    let mut run = Run::new(4, 2, Mode::F32);
    run.cfg.discard_value = -1.0;
    let out = run.eval_ok(&w);
    assert_eq!(px(&out, 0, 0), [-1.0; 4]);
}

#[test]
fn switch_and_select() {
    let w = compile_or_skip!(
        "switch",
        "void main() {
           int k = int(gl_FragCoord.x);
           float r;
           switch (k) { case 0: r = 10.0; break; case 1: case 2: r = 20.0; break; default: r = 30.0; }
           bvec2 b = lessThan(uv, vec2(0.5));
           vec2 s = mix(vec2(0.0), vec2(1.0), b);
           o = vec4(r, s, any(b) ? 1.0 : 0.0);
         }"
    );
    // H = 1, so uv.y = 0.5 everywhere: lessThan(uv.y, 0.5) is false.
    let out = Run::new(4, 1, Mode::F32).eval_ok(&w);
    assert_eq!(px(&out, 0, 0), [10.0, 1.0, 0.0, 1.0]);
    assert_eq!(px(&out, 1, 0), [20.0, 1.0, 0.0, 1.0]);
    assert_eq!(px(&out, 2, 0), [20.0, 0.0, 0.0, 0.0]);
    assert_eq!(px(&out, 3, 0), [30.0, 0.0, 0.0, 0.0]);
}

#[test]
fn helper_function() {
    let w = compile_or_skip!(
        "helper",
        "float sq(float x) { return x * x; }
         void addTo(inout vec2 a, in float b) { a += vec2(b); }
         vec3 twice(vec3 v) { for (int i = 0; i < 2; ++i) v = v + v; return v; }
         void main() {
           vec2 a = vec2(1.0, 2.0);
           addTo(a, sq(3.0));
           addTo(a, 0.5);
           o = vec4(a, twice(vec3(0.25)).xy);
         }"
    );
    let out = Run::new(1, 1, Mode::F32).eval_ok(&w);
    assert_eq!(px(&out, 0, 0), [10.5, 11.5, 1.0, 1.0]);
}

fn image2x2() -> Image {
    let mut img = Image::new(2, 2);
    img.set_texel(0, 0, [1.0, 0.0, 0.0, 1.0]);
    img.set_texel(1, 0, [0.0, 1.0, 0.0, 1.0]);
    img.set_texel(0, 1, [0.0, 0.0, 1.0, 1.0]);
    img.set_texel(1, 1, [1.0, 1.0, 1.0, 0.0]);
    img
}

#[test]
fn bilinear_sampling_2x2() {
    let w = compile_or_skip!(
        "bilinear",
        "layout(set = 0, binding = 0) uniform sampler2D tex;
         void main() { o = texture(tex, uv); }"
    );
    // A 4x4 output over a 2x2 image: pixel (x, y) samples at uv = ((x+0.5)/4, (y+0.5)/4).
    let out = Run::new(4, 4, Mode::F32).sampler("tex", image2x2(), Filter::Linear).eval_ok(&w);
    // Pixel (0,0): u = 0.125*2 - 0.5 = -0.25 -> i0 = -1, a = 0.75; both texels clamp to 0: T00.
    assert_eq!(px(&out, 0, 0), [1.0, 0.0, 0.0, 1.0]);
    // Pixel (3,3): u = 0.875*2 - 0.5 = 1.25 -> i0 = 1, i1 clamps to 1: T11.
    assert_eq!(px(&out, 3, 3), [1.0, 1.0, 1.0, 0.0]);
    // Pixel (1,1): u = v = 0.375*2 - 0.5 = 0.25 -> i0 = j0 = 0, a = b = 0.25.
    // (1-a)(1-b) T00 + a(1-b) T10 + (1-a) b T01 + a b T11
    let (a, b) = (0.25f64, 0.25f64);
    let e = |t00: f64, t10: f64, t01: f64, t11: f64| ((1.0 - a) * (1.0 - b) * t00 + a * (1.0 - b) * t10 + (1.0 - a) * b * t01 + a * b * t11) as f32;
    let expected = [e(1.0, 0.0, 0.0, 1.0), e(0.0, 1.0, 0.0, 1.0), e(0.0, 0.0, 1.0, 1.0), e(1.0, 1.0, 1.0, 0.0)];
    assert_eq!(expected, [0.625, 0.25, 0.25, 0.9375]);
    assert_close(px(&out, 1, 1), expected, 1e-7, "pixel (1,1)");
    // Pixel (2,1): u = 0.625*2 - 0.5 = 0.75 -> a = 0.75, b = 0.25.
    let (a, b) = (0.75f64, 0.25f64);
    let e = |t00: f64, t10: f64, t01: f64, t11: f64| ((1.0 - a) * (1.0 - b) * t00 + a * (1.0 - b) * t10 + (1.0 - a) * b * t01 + a * b * t11) as f32;
    assert_close(px(&out, 2, 1), [e(1.0, 0.0, 0.0, 1.0), e(0.0, 1.0, 0.0, 1.0), e(0.0, 0.0, 1.0, 1.0), e(1.0, 1.0, 1.0, 0.0)], 1e-7, "pixel (2,1)");

    // Nearest: pixel (1,1) has uv 0.375 -> texel floor(0.75) = 0.
    let out = Run::new(4, 4, Mode::F32).sampler("tex", image2x2(), Filter::Nearest).eval_ok(&w);
    assert_eq!(px(&out, 1, 1), [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(px(&out, 2, 1), [0.0, 1.0, 0.0, 1.0]);
    assert_eq!(px(&out, 2, 2), [1.0, 1.0, 1.0, 0.0]);

    // Fixed-point coordinates with 1 fractional bit, round to nearest (the measured NVIDIA behaviour):
    // u = v = 0.25 rounds to 0.5, so a = b = 0.5 -> the mean of all four texels at pixel (1,1).
    let mut run = Run::new(4, 4, Mode::F32).sampler("tex", image2x2(), Filter::Linear);
    run.cfg.sampler_weight_bits = 1;
    let out = run.eval_ok(&w);
    assert_eq!(px(&out, 1, 1), [0.5, 0.5, 0.5, 0.75]);
    // And a coordinate a hair below a texel centre snaps onto it: 8 bits, u = 0.5 - 1e-6 -> a = 0.
    // (Exercised end to end by `lab lift-check`; see lab/lift_tolerances.toml.)
}

#[test]
fn texel_fetch_and_size() {
    let w = compile_or_skip!(
        "fetch",
        "layout(set = 0, binding = 0) uniform sampler2D tex;
         void main() {
           ivec2 sz = textureSize(tex, 0);
           vec4 t = texelFetch(tex, ivec2(gl_FragCoord.xy) % sz, 0);
           o = vec4(t.rgb, float(sz.x * 10 + sz.y));
         }"
    );
    let out = Run::new(3, 3, Mode::F32).sampler("tex", image2x2(), Filter::Linear).eval_ok(&w);
    assert_eq!(px(&out, 0, 0), [1.0, 0.0, 0.0, 22.0]);
    assert_eq!(px(&out, 1, 0), [0.0, 1.0, 0.0, 22.0]);
    assert_eq!(px(&out, 2, 1), [0.0, 0.0, 1.0, 22.0]); // (2 % 2, 1) = (0, 1)
}

#[test]
fn derivatives_of_uv() {
    let w = compile_or_skip!(
        "deriv",
        "void main() { o = vec4(dFdx(uv.x), dFdy(uv.y), fwidth(uv.x + uv.y), dFdxFine(uv.y)); }"
    );
    let (wd, ht) = (6usize, 4usize);
    let out = Run::new(wd, ht, Mode::F32).eval_ok(&w);
    assert_eq!(out.dead_derivatives, 0);
    for y in 0..ht {
        for x in 0..wd {
            let p = px(&out, x, y);
            // dFdx(uv.x) = ((x1+0.5)/W - (x0+0.5)/W) evaluated in f32.
            let dx = ((x | 1) as f32 + 0.5) / wd as f32 - ((x & !1) as f32 + 0.5) / wd as f32;
            let dy = ((y | 1) as f32 + 0.5) / ht as f32 - ((y & !1) as f32 + 0.5) / ht as f32;
            assert_close(p, [dx, dy, dx + dy, 0.0], 1e-7, &format!("pixel {x},{y}"));
            assert!((p[0] - 1.0 / wd as f32).abs() < 1e-6);
            assert!((p[1] - 1.0 / ht as f32).abs() < 1e-6);
        }
    }
    // Odd sizes: the padding lanes outside the image still form full quads.
    let out = Run::new(3, 3, Mode::F32).eval_ok(&w);
    assert!((px(&out, 2, 2)[0] - 1.0 / 3.0).abs() < 1e-6);
}

#[test]
fn derivative_after_discard_in_quad() {
    let w = compile_or_skip!(
        "deriv_discard",
        "void main() { if (uv.x < 0.25) discard; o = vec4(dFdx(uv.x), 0.0, 0.0, 1.0); }"
    );
    // W = 4: pixel 0 (u = 0.125) is discarded, so the left lane of the first quad is dead.
    let out = Run::new(4, 2, Mode::F32).eval_ok(&w);
    assert_eq!(out.discarded_pixels, 2);
    assert_eq!(out.dead_derivatives, 2, "one substituted derivative per surviving lane of the first quad");
    assert_eq!(px(&out, 1, 0)[0], 0.0);
    assert!((px(&out, 2, 0)[0] - 0.25).abs() < 1e-7);
}

#[test]
fn math_functions_match_f64() {
    let w = compile_or_skip!(
        "math",
        "layout(set = 0, binding = 0) uniform Params { float x; float y; float a; float lo; float hi; } u;
         layout(location = 1) out vec4 o1;
         layout(location = 2) out vec4 o2;
         void main() {
           o = vec4(pow(u.x, u.y), exp(u.x), mix(u.x, u.y, u.a), clamp(u.x * 4.0, u.lo, u.hi));
           o1 = vec4(smoothstep(u.lo, u.hi, u.x), sqrt(u.y), inversesqrt(u.y), log2(u.y));
           o2 = vec4(sin(u.x), atan(u.y, u.x), fract(u.y * 3.0), length(vec2(u.x, u.y)));
         }"
    );
    let (x, y, a, lo, hi) = (0.75f32 as f64, 2.5f32 as f64, 0.3f32 as f64, 0.5f32 as f64, 2.0f32 as f64);
    let out = Run::new(2, 2, Mode::F64)
        .uniform("x", "0.75")
        .uniform("y", "2.5")
        .uniform("a", "0.3")
        .uniform("lo", "0.5")
        .uniform("hi", "2.0")
        .eval_ok(&w);
    let got = px(&out, 1, 0);
    let exp = [x.powf(y) as f32, x.exp() as f32, (x * (1.0 - a) + y * a) as f32, (x * 4.0).clamp(lo, hi) as f32];
    assert_eq!(got, exp, "pow/exp/mix/clamp in f64 mode must equal Rust f64 math");
    let t = ((x - lo) / (hi - lo)).clamp(0.0, 1.0);
    let got1 = out.outputs[&1].texel(0, 1);
    assert_eq!(got1, [(t * t * (3.0 - 2.0 * t)) as f32, y.sqrt() as f32, (1.0 / y.sqrt()) as f32, y.log2() as f32]);
    let got2 = out.outputs[&2].texel(1, 1);
    assert_eq!(got2, [x.sin() as f32, y.atan2(x) as f32, (y * 3.0 - (y * 3.0).floor()) as f32, (x * x + y * y).sqrt() as f32]);

    // f32 mode: IEEE f32 for every step.
    let out = Run::new(1, 1, Mode::F32)
        .uniform("x", "0.75")
        .uniform("y", "2.5")
        .uniform("a", "0.3")
        .uniform("lo", "0.5")
        .uniform("hi", "2.0")
        .eval_ok(&w);
    let (xf, yf, af) = (0.75f32, 2.5f32, 0.3f32);
    assert_eq!(px(&out, 0, 0), [xf.powf(yf), xf.exp(), xf * (1.0 - af) + yf * af, (xf * 4.0).clamp(0.5, 2.0)]);
}

#[test]
fn f16_mode_rounds_results() {
    let w = compile_or_skip!(
        "f16",
        "layout(set = 0, binding = 0) uniform Params { float a; float b; } u;
         void main() { o = vec4(u.a + u.b, u.a * u.b, u.a * 131008.0, 1.0 / 3.0); }"
    );
    let b = 2f64.powi(-12);
    let run = || Run::new(1, 1, Mode::F16).uniform("a", "1.0").uniform("b", &format!("{b}"));
    let out = run().eval_ok(&w);
    let p = px(&out, 0, 0);
    assert_eq!(p[0], 1.0, "1 + 2^-12 rounds to 1 in f16");
    assert_eq!(p[1], half::f16::from_f32(b as f32).to_f32(), "product is exactly representable");
    assert!(p[2].is_infinite(), "1.0 * 131008 overflows f16 to inf, got {}", p[2]);
    assert_eq!(p[3], 1.0 / 3.0, "constants are folded by glslang; no arithmetic, no rounding");
    // The same shader in f32 mode keeps the bit.
    let out = Run::new(1, 1, Mode::F32).uniform("a", "1.0").uniform("b", &format!("{b}")).eval_ok(&w);
    assert_eq!(px(&out, 0, 0)[0], 1.0 + b as f32);
}

#[test]
fn matrices_and_integers() {
    let w = compile_or_skip!(
        "matrix",
        "layout(set = 0, binding = 0) uniform Params { mat4 m; ivec2 iv; uint k; } u;
         void main() {
           vec4 v = u.m * vec4(1.0, 2.0, 3.0, 1.0);
           mat2 r = mat2(1.0, 2.0, 3.0, 4.0);
           vec2 w2 = vec2(1.0, 1.0) * r;             // row vector times matrix
           mat2 rt = transpose(r) * r;
           int q = u.iv.x / u.iv.y + u.iv.x % u.iv.y;
           uint bits = (u.k << 3) | (u.k >> 1) ^ 5u;
           o = vec4(v.x + v.w, w2.x + w2.y + determinant(r), float(q) + rt[1][1], float(bits) + float(bitCount(u.k)));
         }"
    );
    // Column-major: column 0 = (1,0,0,0), column 3 = (10, 20, 30, 1).
    let m = "[1,0,0,0, 0,1,0,0, 0,0,1,0, 10,20,30,1]";
    let out = Run::new(1, 1, Mode::F32).uniform("m", m).uniform("iv", "[7,-2]").uniform("k", "6").eval_ok(&w);
    // v = (1+10, 2+20, 3+30, 1) -> v.x + v.w = 12.
    // w2 = (1*1 + 1*2, 1*3 + 1*4) = (3, 7) -> 10, det(r) = 1*4 - 2*3 = -2 -> 8.
    // q = 7 / -2 + 7 % -2: glslang emits OpSDiv (-3) and OpSMod (-1, the divisor's sign) -> -4;
    // rt[1][1] = dot(col1, col1) = 9 + 16 = 25 -> 21.
    // bits = (48 | 3) ^ 5 = 51 ^ 5 = 54; bitCount(6) = 2 -> 56.
    assert_eq!(px(&out, 0, 0), [12.0, 8.0, 21.0, 56.0]);
    let out = Run::new(1, 1, Mode::F32).uniform("m", m).uniform("iv", "[7,2]").uniform("k", "6").eval_ok(&w);
    assert_eq!(px(&out, 0, 0)[2], 3.0 + 1.0 + 25.0);
}

#[test]
fn error_messages() {
    let w = compile_or_skip!(
        "errors",
        "layout(set = 0, binding = 0) uniform Params { float exposure; vec2 texel; } u;
         layout(set = 0, binding = 1) uniform sampler2D tex;
         void main() { o = texture(tex, uv) * u.exposure + vec4(u.texel, 0.0, 0.0); }"
    );
    let e = Run::new(1, 1, Mode::F32).sampler("tex", image2x2(), Filter::Linear).uniform("exposure", "1").eval(&w).unwrap_err();
    let msg = format!("{e:#}");
    assert!(msg.contains("missing values for texel") && msg.contains("exposure: float") && msg.contains("texel: vec2"), "{msg}");
    let e = Run::new(1, 1, Mode::F32).uniform("exposure", "1").uniform("texel", "[1,2]").eval(&w).unwrap_err();
    assert!(format!("{e:#}").contains("no --sampler for tex"), "{e:#}");
    let e = Run::new(1, 1, Mode::F32)
        .sampler("tex", image2x2(), Filter::Linear)
        .uniform("exposure", "1")
        .uniform("texel", "[1,2]")
        .uniform("bogus", "1")
        .eval(&w)
        .unwrap_err();
    assert!(format!("{e:#}").contains("--uniform bogus does not name"), "{e:#}");
    let e = Run::new(1, 1, Mode::F32)
        .sampler("tex", image2x2(), Filter::Linear)
        .uniform("exposure", "1")
        .uniform("texel", "[1,2,3]")
        .eval(&w)
        .unwrap_err();
    assert!(format!("{e:#}").contains("too many numbers"), "{e:#}");
}

#[test]
fn stripped_names_use_binding_and_member_keys() {
    let src = format!(
        "{PRELUDE}layout(set = 0, binding = 2) uniform Params {{ float exposure; vec2 texel; }} u;
         layout(set = 0, binding = 1) uniform sampler2D tex;
         void main() {{ o = texture(tex, uv) * u.exposure + vec4(u.texel, 0.0, 0.0); }}"
    );
    let Some(w) = compile_src("stripped", &src, true) else { return };
    // -g0 strips OpName: names cannot match, the fallback keys do.
    let out = Run::new(1, 1, Mode::F32)
        .sampler("binding1", image2x2(), Filter::Linear)
        .uniform("member0", "2")
        .uniform("offset8", "[0.5,0.25]")
        .eval_ok(&w);
    // W = H = 1: uv = (0.5, 0.5) is the image center, the average of the four texels.
    assert_eq!(px(&out, 0, 0), [2.0 * 0.5 + 0.5, 2.0 * 0.5 + 0.25, 2.0 * 0.5, 2.0 * 0.75]);
    let e = Run::new(1, 1, Mode::F32).sampler("tex", image2x2(), Filter::Linear).uniform("exposure", "2").uniform("texel", "[0.5,0.25]").eval(&w).unwrap_err();
    // Names do not match a stripped module; the error names the usable key.
    assert!(format!("{e:#}").contains("provide --sampler binding1="), "{e:#}");
    let e = Run::new(1, 1, Mode::F32).sampler("binding1", image2x2(), Filter::Linear).uniform("exposure", "2").uniform("texel", "[0.5,0.25]").eval(&w).unwrap_err();
    assert!(format!("{e:#}").contains("member0") && format!("{e:#}").contains("offset8"), "{e:#}");
}

#[test]
fn unsupported_opcode_is_named() {
    let src = format!(
        "{PRELUDE}layout(set = 0, binding = 0) uniform sampler2D tex;
         void main() {{ o = textureProj(tex, vec3(uv, 2.0)); }}"
    );
    let Some(w) = compile_src("unsupported", &src, false) else { return };
    let e = Run::new(2, 2, Mode::F32).sampler("tex", image2x2(), Filter::Linear).eval(&w).unwrap_err();
    let msg = format!("{e:#}");
    assert!(msg.contains("unsupported opcode OpImageSampleProjImplicitLod"), "{msg}");
    assert!(msg.contains("<shader>"), "error names the shader: {msg}");
}

#[test]
fn tonemap_fixture_matches_hand_computation() {
    let Some(w) = common::compile_file(Path::new(common::FIXTURE), false) else { return };
    // Constant image: every sample returns the same color, so the result is closed-form.
    let img = Image::filled(8, 8, 0.0);
    let mut img = img;
    for p in 0..64 {
        img.data[p * 4..p * 4 + 4].copy_from_slice(&[0.4, 0.2, 0.1, 1.0]);
    }
    let run = |mode| Run::new(6, 4, mode).sampler("tex", img.clone(), Filter::Linear).uniform("exposure", "1.5").uniform("gamma", "2.2").uniform("texel", "[0.01,0.02]");
    let out = run(Mode::F64).eval_ok(&w);
    assert_eq!(out.discarded_pixels, 0);
    let expected: Vec<f32> = [0.4f32, 0.2, 0.1]
        .iter()
        .map(|&t| {
            let t = t as f64;
            let c = t * 1.5f32 as f64;
            let blur = t * 5.0;
            let c = c * (1.0 - 0.25) + blur * 0.2 * 0.25;
            let (a, b, cc, d, e) = (2.51f32 as f64, 0.03f32 as f64, 2.43f32 as f64, 0.59f32 as f64, 0.14f32 as f64);
            let c = ((c * (a * c + b)) / (c * (cc * c + d) + e)).clamp(0.0, 1.0);
            c.powf(1.0 / 2.2f32 as f64) as f32
        })
        .collect();
    assert_close(px(&out, 3, 1), [expected[0], expected[1], expected[2], 1.0], 2e-6, "tonemap f64");
    let out32 = run(Mode::F32).eval_ok(&w);
    assert_close(px(&out32, 3, 1), [expected[0], expected[1], expected[2], 1.0], 2e-6, "tonemap f32");
    // Zero exposure and a black image: luminance < 0.001 -> every pixel discarded.
    let black = Image::new(2, 2);
    let out = Run::new(2, 2, Mode::F32).sampler("tex", black, Filter::Linear).uniform("exposure", "0").uniform("gamma", "2.2").uniform("texel", "[0,0]").eval_ok(&w);
    assert_eq!(out.discarded_pixels, 4);
}

#[test]
fn private_globals_and_arrays() {
    let w = compile_or_skip!(
        "arrays",
        "float acc = 1.0;
         const float table[3] = float[3](1.0, 2.0, 4.0);
         struct S { vec2 p; float w; };
         void bump(float f) { acc *= f; }
         void main() {
           float arr[4];
           for (int i = 0; i < 4; ++i) arr[i] = float(i) * 0.5;
           S s = S(vec2(arr[3], arr[2]), table[int(gl_FragCoord.x) % 3]);
           bump(s.w); bump(2.0);
           vec3 v = vec3(1.0, 2.0, 3.0); v.zx = vec2(9.0, 8.0);
           o = vec4(s.p, acc, v.x + v.y * 10.0 + v.z * 100.0);
         }"
    );
    let out = Run::new(3, 1, Mode::F32).eval_ok(&w);
    assert_eq!(px(&out, 0, 0), [1.5, 1.0, 2.0, 8.0 + 20.0 + 900.0]);
    assert_eq!(px(&out, 2, 0), [1.5, 1.0, 8.0, 928.0]);
}
