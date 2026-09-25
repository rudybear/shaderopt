//! `shader-ir rewrite`: the M2 rewrite command (`lab/CONTRACTS.md`).
//!
//! ```text
//! shader-ir rewrite --spv in.spv --out out.spv --passes fold,dce,cse,ident,unroll,divconst,powspec,select \
//!     --ops ops.json [--max-unroll 16] [--max-select-arm 32] [--only-op ID]
//! ```

use anyhow::{bail, Context, Result};
use clap::Args;
use shader_ir::lift::Lifted;
use shader_ir::passes::{self, Opts};
use shader_ir::{bytes_from_words, read_spv};
use std::path::PathBuf;

#[derive(Args)]
pub struct RewriteArgs {
    #[arg(long)]
    pub spv: PathBuf,
    #[arg(long)]
    pub out: PathBuf,
    /// Comma-separated pass list, applied in order; `fold,dce,cse,ident` among them are
    /// repeated to a fixed point after every other pass.
    #[arg(long)]
    pub passes: String,
    /// Write the EditOp records here (JSON array).
    #[arg(long)]
    pub ops: Option<PathBuf>,
    /// `unroll`: maximum trip count.
    #[arg(long, default_value_t = 16)]
    pub max_unroll: u32,
    /// `select`: maximum instructions per arm.
    #[arg(long, default_value_t = 32)]
    pub max_select_arm: usize,
    /// Restrict the passes to one target id (`dce` is unrestricted).
    #[arg(long)]
    pub only_op: Option<u32>,
    /// Skip every edit that would be classed `ulp`.
    #[arg(long, default_value_t = false)]
    pub exact_only: bool,
}

pub fn run(a: RewriteArgs) -> Result<()> {
    let words = read_spv(&a.spv)?;
    let mut lifted = Lifted::load(&words)?;
    let passes: Vec<String> = a.passes.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    if passes.is_empty() {
        bail!("--passes is empty; passes are {}", passes::ALL_PASSES.join(","));
    }
    let opts = Opts { max_unroll: a.max_unroll, max_select_arm: a.max_select_arm, only_op: a.only_op, exact_only: a.exact_only, ..Opts::default() };
    let before = passes::instruction_count(&lifted);
    let bound_before = lifted.bound;
    let mut ops = Vec::new();
    let counts = passes::run_pipeline(&mut lifted, &passes, &mut ops, &opts)?;
    let out_words = lifted.assemble();
    passes::validate(&out_words).with_context(|| format!("{}: rewritten module does not validate", a.spv.display()))?;
    std::fs::write(&a.out, bytes_from_words(&out_words)).with_context(|| format!("cannot write {}", a.out.display()))?;
    if let Some(p) = &a.ops {
        let arr: Vec<serde_json::Value> = ops.iter().map(|o| o.to_json()).collect();
        std::fs::write(p, serde_json::to_string_pretty(&serde_json::Value::Array(arr))?)
            .with_context(|| format!("cannot write {}", p.display()))?;
    }
    let after = passes::instruction_count(&lifted);
    let exact = ops.iter().filter(|o| o.class == passes::Class::Exact).count();
    println!(
        "{}: instructions {before} -> {after}, bound {bound_before} -> {}, {} ops ({exact} exact, {} ulp); wrote {}",
        a.spv.display(),
        lifted.bound,
        ops.len(),
        ops.len() - exact,
        a.out.display()
    );
    for (name, n) in &counts {
        println!("  {name:<9} {n}");
    }
    Ok(())
}
