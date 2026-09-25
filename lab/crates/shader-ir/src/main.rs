//! `shader-ir` command line, as specified in `lab/CONTRACTS.md`.

mod cli_demote;
mod cli_rewrite;

use anyhow::{anyhow, Context, Result};
use clap::{Args, Parser, Subcommand};
use shader_ir::interp::{self, EvalConfig, Filter, Mode, SamplerSpec};
use shader_ir::lift::{self, Lifted, Type};
use shader_ir::{bytes_from_words, npy, read_spv};
use spirv::StorageClass;
use std::path::PathBuf;

mod cli_analyze;
mod cli_approx;
mod cli_hoist;

#[derive(Parser)]
#[command(name = "shader-ir", version, about = "Shader lab CPU model: lossless SPIR-V lift and fragment interpreter")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Load and re-assemble a module; exit 0 iff the body after the 5-word header is identical.
    Roundtrip {
        input: PathBuf,
        /// Write the re-assembled module here.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Evaluate the Fragment entry point at every pixel center and write a float32 RGBA .npy.
    Eval(EvalArgs),
    /// Print the entry point, interface variables, uniform block layout and an opcode histogram.
    Info { input: PathBuf },
    /// Apply the M2 rewrite passes and validate the result (see lab/CONTRACTS.md).
    Rewrite(cli_rewrite::RewriteArgs),
    /// M2 static analysis: rates, sinks, sampler coordinates, source lines, ranges -> JSON.
    Analyze(cli_analyze::AnalyzeArgs),
    /// M4: hoist maximal uniform-rate subtrees into new uniform block members (see lab/CONTRACTS.md).
    Hoist(cli_hoist::HoistArgs),
    /// M4: replace GLSL.std.450 transcendentals at listed sites by range-fitted polynomials.
    Approx(cli_approx::ApproxArgs),
    /// M3 precision demotion: RelaxedPrecision decorations or explicit f16 for listed sites.
    Demote(cli_demote::DemoteArgs),
}

#[derive(Args)]
struct EvalArgs {
    #[arg(long)]
    spv: PathBuf,
    #[arg(long)]
    width: usize,
    #[arg(long)]
    height: usize,
    /// f32 | f64 | f16
    #[arg(long, default_value = "f32")]
    mode: String,
    /// name=path.npy[:nearest|:linear] (repeatable); matched by the sampler variable's OpName.
    #[arg(long = "sampler", value_name = "NAME=PATH[:FILTER]")]
    samplers: Vec<String>,
    /// name=value (repeatable): uniform block member by name; `1.0`, `3`, `[1,2,3]`, 16 numbers
    /// column-major for a mat4.
    #[arg(long = "uniform", value_name = "NAME=VALUE")]
    uniforms: Vec<String>,
    /// name=value (repeatable): constant value for an extra Location input variable.
    #[arg(long = "input", value_name = "NAME=VALUE")]
    inputs: Vec<String>,
    /// Output .npy for the location chosen by --out-location.
    #[arg(long)]
    out: PathBuf,
    #[arg(long, default_value_t = 0)]
    out_location: u32,
    /// Value written to all channels of discarded pixels (`nan` or a number).
    #[arg(long, default_value = "nan")]
    discard_value: String,
    /// Quantize bilinear weights to N fractional bits (0 = full precision).
    #[arg(long, default_value_t = 0)]
    sampler_weight_bits: u32,
    /// Number of worker threads (default: all cores).
    #[arg(long)]
    threads: Option<usize>,
    /// Write per-result float ranges (min/max/nan/inf/samples by result id) to this JSON file.
    #[arg(long)]
    profile: Option<PathBuf>,
    /// Evaluate every N-th quad in each dimension (quads at (2iN, 2jN) run whole).
    #[arg(long, default_value_t = 1)]
    stride: usize,
    /// Result ids to round to f16 after computing (comma separated): predicts demoting those
    /// sites to RelaxedPrecision / explicit f16.
    #[arg(long, value_delimiter = ',')]
    f16_sites: Vec<u32>,
    /// Round every float-typed result to f16.
    #[arg(long)]
    f16_all: bool,
    /// Result ids whose values at pixel (0, 0) are written to --dump (M4 hoist values; run on
    /// the module given to `hoist`, any tiny size such as --width 2 --height 2).
    #[arg(long, value_delimiter = ',')]
    dump_ids: Vec<u32>,
    /// JSON file for --dump-ids: {"<id>": [components as f64]}.
    #[arg(long)]
    dump: Option<PathBuf>,
}

fn split_kv<'a>(s: &'a str, what: &str) -> Result<(&'a str, &'a str)> {
    s.split_once('=').ok_or_else(|| anyhow!("--{what} {s:?}: expected name=value"))
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e:#}");
        std::process::exit(2);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Roundtrip { input, out } => {
            let words = read_spv(&input)?;
            let (_lifted, out_words, rt) = lift::roundtrip(&words)?;
            println!(
                "{}: body_identical={} words_in={} words_out={}",
                input.display(),
                rt.body_identical,
                rt.words_in,
                rt.words_out
            );
            let diff = rt.header_diff();
            if diff.is_empty() {
                println!("header: identical");
            } else {
                for d in diff {
                    println!("{d}");
                }
            }
            if !rt.body_identical {
                println!("first differing word: {:?}", rt.first_diff_word);
            }
            if let Some(out) = out {
                std::fs::write(&out, bytes_from_words(&out_words)).with_context(|| format!("cannot write {}", out.display()))?;
                println!("wrote {}", out.display());
            }
            if !rt.body_identical {
                std::process::exit(1);
            }
            Ok(())
        }
        Cmd::Rewrite(a) => cli_rewrite::run(a),
        Cmd::Info { input } => {
            let words = read_spv(&input)?;
            let lifted = Lifted::load(&words)?;
            print_info(&input, &words, &lifted);
            Ok(())
        }
        Cmd::Analyze(a) => cli_analyze::run(&a),
        Cmd::Hoist(a) => cli_hoist::run(a),
        Cmd::Approx(a) => cli_approx::run(a),
        Cmd::Demote(a) => cli_demote::run(a),
        Cmd::Eval(a) => {
            if let Some(n) = a.threads {
                rayon::ThreadPoolBuilder::new().num_threads(n).build_global().ok();
            }
            let words = read_spv(&a.spv)?;
            let lifted = Lifted::load(&words)?;
            let mode: Mode = a.mode.parse()?;
            let mut cfg = EvalConfig::new(a.width, a.height, mode);
            cfg.label = a.spv.display().to_string();
            cfg.sampler_weight_bits = a.sampler_weight_bits;
            cfg.profile = a.profile.is_some();
            cfg.stride = a.stride.max(1);
            cfg.f16_sites = a.f16_sites.clone();
            cfg.f16_all = a.f16_all;
            cfg.discard_value = match a.discard_value.as_str() {
                "nan" | "NaN" => f32::NAN,
                s => s.parse().map_err(|e| anyhow!("--discard-value {s:?}: {e}"))?,
            };
            for s in &a.samplers {
                let (name, rest) = split_kv(s, "sampler")?;
                let (path, filter) = match rest.rsplit_once(':') {
                    Some((p, f)) if f == "nearest" || f == "linear" => (p, f.parse::<Filter>()?),
                    _ => (rest, Filter::Linear),
                };
                let image = npy::read(std::path::Path::new(path))?;
                cfg.samplers.push(SamplerSpec { name: name.to_string(), image, filter });
            }
            for u in &a.uniforms {
                let (n, v) = split_kv(u, "uniform")?;
                cfg.uniforms.push((n.to_string(), v.to_string()));
            }
            for u in &a.inputs {
                let (n, v) = split_kv(u, "input")?;
                cfg.inputs.push((n.to_string(), v.to_string()));
            }
            if let Some(p) = &a.dump {
                let d = interp::dump::dump_values(&lifted, &cfg, &a.dump_ids)?;
                std::fs::write(p, serde_json::to_string_pretty(&interp::dump::dump_json(&d))?).with_context(|| format!("cannot write {}", p.display()))?;
                if !d.never_executed.is_empty() {
                    eprintln!("{}: warning: ids {:?} were never executed on {}x{} (zero-filled)", a.spv.display(), d.never_executed, a.width, a.height);
                }
                eprintln!("{}: wrote {} values (last from pixel {:?}) to {}", a.spv.display(), d.values.len(), d.pixel, p.display());
            }
            let t0 = std::time::Instant::now();
            let out = interp::evaluate(&lifted, &cfg)?;
            let dt = t0.elapsed();
            let img = out.outputs.get(&a.out_location).ok_or_else(|| {
                anyhow!(
                    "no output at Location {}; outputs are at {:?}",
                    a.out_location,
                    out.outputs.keys().collect::<Vec<_>>()
                )
            })?;
            npy::write(&a.out, img)?;
            if let (Some(p), Some(r)) = (&a.profile, &out.ranges) {
                let text = serde_json::to_string(&shader_ir::analysis::ranges_json(r))?;
                std::fs::write(p, text).with_context(|| format!("cannot write {}", p.display()))?;
                eprintln!("{}: wrote ranges for {} float results to {}", a.spv.display(), r.len(), p.display());
            }
            eprintln!(
                "{}: {}x{} mode={:?} in {:.3}s; discarded={} dead_derivatives={}; wrote {}",
                a.spv.display(),
                a.width,
                a.height,
                mode,
                dt.as_secs_f64(),
                out.discarded_pixels,
                out.dead_derivatives,
                a.out.display()
            );
            Ok(())
        }
    }
}

fn print_info(path: &std::path::Path, words: &[u32], l: &Lifted) {
    let h = l.module.header.as_ref();
    println!("module: {} ({} words, bound {})", path.display(), words.len(), l.bound);
    if let Some(h) = h {
        println!(
            "header: version {}.{} generator {:#x} schema {}",
            (h.version >> 16) & 0xff,
            (h.version >> 8) & 0xff,
            h.generator,
            h.reserved_word
        );
    }
    for ep in &l.entry_points {
        println!("entry point: {:?} {:?} (function %{}, {} interface ids)", ep.name, ep.model, ep.function, ep.interface.len());
    }
    let mut n_inst = 0;
    for f in &l.module.functions {
        for b in &f.blocks {
            n_inst += b.instructions.len();
        }
    }
    println!(
        "functions: {} ({} blocks, {} instructions)",
        l.functions.len(),
        l.functions.iter().map(|f| f.blocks.len()).sum::<usize>(),
        n_inst
    );
    for f in &l.functions {
        println!(
            "  %{} {}: {} blocks, {} loop headers, {} selection headers",
            f.id,
            f.name.as_deref().unwrap_or("?"),
            f.blocks.len(),
            f.headers.values().filter(|h| h.continue_target.is_some()).count(),
            f.headers.values().filter(|h| h.continue_target.is_none()).count()
        );
    }
    println!("interface variables:");
    for v in &l.variables {
        let mut attrs = Vec::new();
        if let Some(x) = v.location {
            attrs.push(format!("location={x}"));
        }
        if let Some(x) = v.descriptor_set {
            attrs.push(format!("set={x}"));
        }
        if let Some(x) = v.binding {
            attrs.push(format!("binding={x}"));
        }
        if let Some(b) = v.builtin {
            attrs.push(format!("builtin={b:?}"));
        }
        if v.block {
            attrs.push("block".into());
        }
        if v.relaxed_precision {
            attrs.push("relaxed".into());
        }
        println!(
            "  %{} {:<16} {:?} {} {}",
            v.id,
            v.name.as_deref().unwrap_or("?"),
            v.storage,
            l.type_name(v.pointee),
            attrs.join(" ")
        );
        if matches!(v.storage, StorageClass::Uniform | StorageClass::PushConstant) {
            if let Some(Type::Struct { members }) = l.types.get(&v.pointee) {
                for (m, mty) in members.iter().enumerate() {
                    let name = l.member_name(v.pointee, m as u32).unwrap_or("?");
                    let off = l.member_decoration_u32(v.pointee, m as u32, spirv::Decoration::Offset);
                    let stride = l.decoration_u32(*mty, spirv::Decoration::ArrayStride);
                    let mstride = l.member_decoration_u32(v.pointee, m as u32, spirv::Decoration::MatrixStride);
                    let mut extra = String::new();
                    if let Some(s) = stride {
                        extra.push_str(&format!(" array_stride={s}"));
                    }
                    if let Some(s) = mstride {
                        extra.push_str(&format!(" matrix_stride={s}"));
                    }
                    if l.member_decorations.get(&(v.pointee, m as u32)).map_or(false, |d| d.iter().any(|x| x.decoration == spirv::Decoration::RelaxedPrecision)) {
                        extra.push_str(" relaxed");
                    }
                    println!(
                        "      member {m} {:<14} {:<10} offset={}{}",
                        name,
                        l.type_name(*mty),
                        off.map(|o| o.to_string()).unwrap_or("?".into()),
                        extra
                    );
                }
            }
        }
    }
    println!("opcode histogram:");
    for (name, n) in l.opcode_histogram() {
        println!("  {n:>6} {name}");
    }
}

