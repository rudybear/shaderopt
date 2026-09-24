//! Shared helpers for the integration tests: compile GLSL with the pinned glslang, skip when
//! the tools are missing.

#![allow(dead_code)]

use shader_ir::interp::{self, EvalConfig, EvalOutput, Filter, Mode, SamplerSpec};
use shader_ir::lift::Lifted;
use shader_ir::npy::Image;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const GLSLANG: &str = "/home/rudybear/sources/igl/third-party/deps/src/glslang/build/StandAlone/glslang";
pub const SPIRV_VAL: &str = "/home/rudybear/sources/igl/third-party/deps/src/glslang/build/External/spirv-tools/tools/spirv-val";
pub const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/tonemap.frag");
pub const CORPUS_DIR: &str = "/home/rudybear/sources/shaderopt/lab/shaders";

pub fn tmp_dir() -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("t{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

pub fn glslang_available() -> bool {
    if Path::new(GLSLANG).is_file() {
        true
    } else {
        eprintln!("SKIP: glslang not found at {GLSLANG}");
        false
    }
}

/// Compiles a GLSL fragment shader; `strip` adds `-g0` (the CONTRACTS.md compile line).
/// Returns `None` when glslang is absent (the caller skips), panics on a compile error.
pub fn compile_file(frag: &Path, strip: bool) -> Option<Vec<u32>> {
    if !glslang_available() {
        return None;
    }
    let out = tmp_dir().join(format!(
        "{}{}.spv",
        frag.file_stem().unwrap().to_string_lossy(),
        if strip { "_g0" } else { "" }
    ));
    let mut cmd = Command::new(GLSLANG);
    cmd.arg("-V");
    if strip {
        cmd.arg("-g0");
    }
    let st = cmd.arg("-o").arg(&out).arg(frag).output().expect("run glslang");
    assert!(
        st.status.success(),
        "glslang failed on {}:\n{}{}",
        frag.display(),
        String::from_utf8_lossy(&st.stdout),
        String::from_utf8_lossy(&st.stderr)
    );
    Some(shader_ir::read_spv(&out).unwrap())
}

pub fn compile_src(name: &str, src: &str, strip: bool) -> Option<Vec<u32>> {
    let p = tmp_dir().join(format!("{name}.frag"));
    std::fs::write(&p, src).unwrap();
    compile_file(&p, strip)
}

/// Runs spirv-val on words; returns None when the tool is missing.
pub fn validate(words: &[u32]) -> Option<bool> {
    if !Path::new(SPIRV_VAL).is_file() {
        eprintln!("SKIP: spirv-val not found at {SPIRV_VAL}");
        return None;
    }
    let p = tmp_dir().join("val.spv");
    std::fs::write(&p, shader_ir::bytes_from_words(words)).unwrap();
    let st = Command::new(SPIRV_VAL).arg(&p).output().expect("run spirv-val");
    if !st.status.success() {
        eprintln!("spirv-val: {}", String::from_utf8_lossy(&st.stderr));
    }
    Some(st.status.success())
}

pub struct Run {
    pub cfg: EvalConfig,
}

impl Run {
    pub fn new(w: usize, h: usize, mode: Mode) -> Self {
        Run { cfg: EvalConfig::new(w, h, mode) }
    }
    pub fn uniform(mut self, name: &str, value: &str) -> Self {
        self.cfg.uniforms.push((name.into(), value.into()));
        self
    }
    pub fn sampler(mut self, name: &str, image: Image, filter: Filter) -> Self {
        self.cfg.samplers.push(SamplerSpec { name: name.into(), image, filter });
        self
    }
    pub fn eval(self, words: &[u32]) -> anyhow::Result<EvalOutput> {
        let lifted = Lifted::load(words)?;
        interp::evaluate(&lifted, &self.cfg)
    }
    pub fn eval_ok(self, words: &[u32]) -> EvalOutput {
        self.eval(words).unwrap_or_else(|e| panic!("eval failed: {e:#}"))
    }
}

pub fn px(out: &EvalOutput, x: usize, y: usize) -> [f32; 4] {
    out.outputs[&0].texel(x, y)
}

pub fn assert_close(actual: [f32; 4], expected: [f32; 4], tol: f32, what: &str) {
    for i in 0..4 {
        let (a, e) = (actual[i], expected[i]);
        assert!(
            (a - e).abs() <= tol || (a.is_nan() && e.is_nan()),
            "{what}: channel {i}: got {a}, expected {e} (tol {tol}); full {actual:?} vs {expected:?}"
        );
    }
}

/// The standard preamble for the tiny test shaders.
pub const PRELUDE: &str = "#version 450\nlayout(location = 0) in vec2 uv;\nlayout(location = 0) out vec4 o;\n";
