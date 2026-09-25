//! `shader-ir analyze`: the M2 static analysis CLI (see `lab/CONTRACTS.md`).

use anyhow::{Context, Result};
use clap::Args;
use shader_ir::analysis::{self, AnalyzeOptions};
use shader_ir::lift::Lifted;
use shader_ir::read_spv;
use std::path::PathBuf;

#[derive(Args)]
pub struct AnalyzeArgs {
    /// The measured (`glslang -V`) module; its result ids are the canonical ones.
    #[arg(long)]
    pub spv: PathBuf,
    /// The `glslang -V -g` build of the same shader, for source lines.
    #[arg(long)]
    pub debug_spv: Option<PathBuf>,
    /// `ranges.json` written by `eval --profile`.
    #[arg(long)]
    pub ranges: Option<PathBuf>,
    /// Output JSON.
    #[arg(long)]
    pub out: PathBuf,
}

pub fn run(a: &AnalyzeArgs) -> Result<()> {
    let words = read_spv(&a.spv)?;
    let lifted = Lifted::load(&words).with_context(|| format!("{}", a.spv.display()))?;
    let debug = match &a.debug_spv {
        Some(p) => Some(Lifted::load(&read_spv(p)?).with_context(|| format!("{}", p.display()))?),
        None => None,
    };
    let ranges = match &a.ranges {
        Some(p) => Some(analysis::parse_ranges(&std::fs::read_to_string(p).with_context(|| format!("cannot read {}", p.display()))?)?),
        None => None,
    };
    let shader = a.spv.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let shader = shader.strip_suffix(".g").map(|s| s.to_string()).unwrap_or(shader);
    let opts = AnalyzeOptions { shader, debug: debug.as_ref(), ranges: ranges.as_ref() };
    let result = analysis::analyze(&lifted, &opts).with_context(|| format!("{}", a.spv.display()))?;
    let json = serde_json::to_string_pretty(&result.to_json())?;
    std::fs::write(&a.out, json).with_context(|| format!("cannot write {}", a.out.display()))?;
    let s = &result.summary;
    eprintln!(
        "{}: {} instructions; pixel={} uniform={} const={} sink_sites={} float_sites={} candidate_sites={}; wrote {}",
        a.spv.display(),
        result.instructions.len(),
        s.pixel,
        s.uniform,
        s.const_,
        s.sink_sites,
        s.float_sites,
        s.candidate_sites,
        a.out.display()
    );
    Ok(())
}
