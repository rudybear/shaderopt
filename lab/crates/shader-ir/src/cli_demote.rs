//! `shader-ir demote`: the M3 precision-demotion command (`lab/CONTRACTS.md`, "M3").
//!
//! ```text
//! shader-ir demote --spv in.spv --out out.spv --sites 57,58,90 --mode relaxed|f16 --ops ops.json \
//!     [--group-converts]
//! ```

use anyhow::{Context, Result};
use clap::Args;
use shader_ir::lift::Lifted;
use shader_ir::passes::{self, demote};
use shader_ir::{bytes_from_words, read_spv};
use std::path::PathBuf;

#[derive(Args)]
pub struct DemoteArgs {
    #[arg(long)]
    pub spv: PathBuf,
    #[arg(long)]
    pub out: PathBuf,
    /// Comma-separated f32 float-typed result ids of the input module (scalars or vectors).
    #[arg(long, value_delimiter = ',', required = true)]
    pub sites: Vec<u32>,
    /// `relaxed`: OpDecorate RelaxedPrecision per site; `f16`: compute the sites in f16.
    #[arg(long)]
    pub mode: String,
    /// Write the EditOp records here (JSON array).
    #[arg(long)]
    pub ops: Option<PathBuf>,
    /// After f16 demotion, remove redundant f16->f32->f16 conversion pairs.
    #[arg(long, default_value_t = false)]
    pub group_converts: bool,
}

pub fn run(a: DemoteArgs) -> Result<()> {
    let words = read_spv(&a.spv)?;
    let mut lifted = Lifted::load(&words)?;
    let mode: demote::DemoteMode = a.mode.parse()?;
    let bound_before = lifted.bound;
    let mut ops = Vec::new();
    let report = demote::run(&mut lifted, &a.sites, mode, a.group_converts, &mut ops).with_context(|| format!("{}", a.spv.display()))?;
    let out_words = lifted.assemble();
    passes::validate(&out_words).with_context(|| format!("{}: demoted module does not validate", a.spv.display()))?;
    std::fs::write(&a.out, bytes_from_words(&out_words)).with_context(|| format!("cannot write {}", a.out.display()))?;
    if let Some(p) = &a.ops {
        let arr: Vec<serde_json::Value> = ops.iter().map(|o| o.to_json()).collect();
        std::fs::write(p, serde_json::to_string_pretty(&serde_json::Value::Array(arr))?)
            .with_context(|| format!("cannot write {}", p.display()))?;
    }
    println!(
        "{}: demote {}: {} of {} sites demoted, {} ops, converts in {} / out {} / removed {}, bound {} -> {}; wrote {}",
        a.spv.display(),
        a.mode,
        report.demoted,
        a.sites.len(),
        ops.len(),
        report.converts_in,
        report.converts_out,
        report.converts_removed,
        bound_before,
        lifted.bound,
        a.out.display()
    );
    for (id, why) in &report.skipped {
        println!("  skipped %{id}: {why}");
    }
    Ok(())
}
