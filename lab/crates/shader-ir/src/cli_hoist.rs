//! `shader-ir hoist`: the M4 uniform-rate hoisting command (`lab/CONTRACTS.md`, "M4").
//!
//! ```text
//! shader-ir hoist --spv in.spv --out out.spv --ops ops.json --plan plan.json [--min-ops 2]
//! ```

use anyhow::{Context, Result};
use clap::Args;
use shader_ir::lift::Lifted;
use shader_ir::passes::{self, hoist, Opts};
use shader_ir::{bytes_from_words, read_spv};
use std::path::PathBuf;

#[derive(Args)]
pub struct HoistArgs {
    #[arg(long)]
    pub spv: PathBuf,
    #[arg(long)]
    pub out: PathBuf,
    /// Write the EditOp records here (JSON array).
    #[arg(long)]
    pub ops: Option<PathBuf>,
    /// Write the hoisting plan here (JSON array of member/type/source_id/expr/depends_on).
    #[arg(long)]
    pub plan: Option<PathBuf>,
    /// Minimum arithmetic/ext-inst instructions in a hoisted subtree.
    #[arg(long, default_value_t = 2)]
    pub min_ops: usize,
}

pub fn run(a: HoistArgs) -> Result<()> {
    let words = read_spv(&a.spv)?;
    let mut lifted = Lifted::load(&words)?;
    let before = passes::instruction_count(&lifted);
    let bound_before = lifted.bound;
    let mut ops = Vec::new();
    let report = hoist::run(&mut lifted, &mut ops, a.min_ops).with_context(|| format!("{}: hoist", a.spv.display()))?;
    let dce = if report.entries.is_empty() { 0 } else { passes::run_pass("dce", &mut lifted, &mut ops, &Opts::default())? };
    let out_words = lifted.assemble();
    passes::validate(&out_words).with_context(|| format!("{}: hoisted module does not validate", a.spv.display()))?;
    std::fs::write(&a.out, bytes_from_words(&out_words)).with_context(|| format!("cannot write {}", a.out.display()))?;
    if let Some(p) = &a.ops {
        let arr: Vec<serde_json::Value> = ops.iter().map(|o| o.to_json()).collect();
        std::fs::write(p, serde_json::to_string_pretty(&serde_json::Value::Array(arr))?).with_context(|| format!("cannot write {}", p.display()))?;
    }
    if let Some(p) = &a.plan {
        let arr: Vec<serde_json::Value> = report.entries.iter().map(|e| e.to_json()).collect();
        std::fs::write(p, serde_json::to_string_pretty(&serde_json::Value::Array(arr))?).with_context(|| format!("cannot write {}", p.display()))?;
    }
    let after = passes::instruction_count(&lifted);
    match &report.block {
        None => println!(
            "{}: no block ({} uniform-rate candidates, nothing hoisted); instructions {before}; wrote {}",
            a.spv.display(),
            report.candidates,
            a.out.display()
        ),
        Some(block) => println!(
            "{}: hoisted {} values into {block} (+{} bytes std140), instructions {before} -> {after}, bound {bound_before} -> {}, {} ops ({} dce, {} dead variables); wrote {}",
            a.spv.display(),
            report.entries.len(),
            report.bytes_added,
            lifted.bound,
            ops.len(),
            dce,
            report.dead_variables,
            a.out.display()
        ),
    }
    for e in &report.entries {
        println!("  {:<8} {:<5} offset={:<4} id={:<5} ops={:<3} {} = {}", e.member, e.ty, e.offset, e.source_id, e.ops, e.depends_on.join(","), e.expr);
    }
    if !report.entries.is_empty() {
        let ids: Vec<String> = report.entries.iter().map(|e| e.source_id.to_string()).collect();
        println!("  values: shader-ir eval --spv {} --width 2 --height 2 ... --dump-ids {} --dump dump.json", a.spv.display(), ids.join(","));
    }
    Ok(())
}
