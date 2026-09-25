//! `shader-ir approx`: the M4 polynomial approximation command (`lab/CONTRACTS.md`, "M4").
//!
//! ```text
//! shader-ir approx --spv in.spv --out out.spv --ops ops.json --ranges ranges.json --sites 90,91 \
//!     [--degree 3..7] [--max-rel-err 1e-3]
//! ```

use anyhow::{bail, Context, Result};
use clap::Args;
use shader_ir::analysis;
use shader_ir::lift::Lifted;
use shader_ir::passes::{self, approx};
use shader_ir::{bytes_from_words, read_spv};
use std::path::PathBuf;

#[derive(Args)]
pub struct ApproxArgs {
    #[arg(long)]
    pub spv: PathBuf,
    #[arg(long)]
    pub out: PathBuf,
    /// Write the EditOp records here (JSON array).
    #[arg(long)]
    pub ops: Option<PathBuf>,
    /// `ranges.json` written by `eval --profile` (the operand ranges are looked up here).
    #[arg(long)]
    pub ranges: PathBuf,
    /// Result ids of the GLSL.std.450 sites to approximate (comma separated).
    #[arg(long, value_delimiter = ',')]
    pub sites: Vec<u32>,
    /// Polynomial degree range `LO..HI` (or a single degree); the smallest that fits is used.
    #[arg(long, default_value = "3..7")]
    pub degree: String,
    /// Maximum relative error of the polynomial over the padded operand range.
    #[arg(long, default_value_t = 1e-3)]
    pub max_rel_err: f64,
}

pub fn run(a: ApproxArgs) -> Result<()> {
    if a.sites.is_empty() {
        bail!("--sites is empty");
    }
    let words = read_spv(&a.spv)?;
    let mut lifted = Lifted::load(&words)?;
    let ranges = analysis::parse_ranges(&std::fs::read_to_string(&a.ranges).with_context(|| format!("cannot read {}", a.ranges.display()))?)?;
    let opts = approx::ApproxOpts { sites: a.sites.clone(), degree: approx::parse_degree(&a.degree)?, max_rel_err: a.max_rel_err };
    let mut ops = Vec::new();
    let reports = approx::run(&mut lifted, &ranges, &opts, &mut ops).with_context(|| format!("{}: approx", a.spv.display()))?;
    let out_words = lifted.assemble();
    passes::validate(&out_words).with_context(|| format!("{}: approximated module does not validate", a.spv.display()))?;
    std::fs::write(&a.out, bytes_from_words(&out_words)).with_context(|| format!("cannot write {}", a.out.display()))?;
    if let Some(p) = &a.ops {
        let arr: Vec<serde_json::Value> = ops.iter().map(|o| o.to_json()).collect();
        std::fs::write(p, serde_json::to_string_pretty(&serde_json::Value::Array(arr))?).with_context(|| format!("cannot write {}", p.display()))?;
    }
    let replaced = reports.iter().filter(|r| r.replaced()).count();
    println!("{}: {replaced} of {} sites replaced ({} ops, all lossy); wrote {}", a.spv.display(), reports.len(), ops.len(), a.out.display());
    println!("  {:<6} {:<14} {:<8} {:<22} {:<6} {:<9} status", "id", "op", "type", "range", "degree", "rel err");
    for r in &reports {
        println!(
            "  {:<6} {:<14} {:<8} {:<22} {:<6} {:<9} {}",
            r.id,
            r.op,
            r.ty,
            r.range.map(|(lo, hi)| format!("[{}, {}]", approx::fmt_g(lo), approx::fmt_g(hi))).unwrap_or("-".into()),
            r.degree.map(|d| d.to_string()).unwrap_or("-".into()),
            r.max_rel_err.map(|e| format!("{e:.1e}")).unwrap_or("-".into()),
            r.status
        );
    }
    Ok(())
}
