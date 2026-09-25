//! `eval --dump-ids 57,58 --dump dump.json`: the values of listed result ids at pixel (0, 0).
//!
//! Only the quad at (0, 0) is evaluated (four lanes, so derivatives work); the values are read
//! from lane 0 after the invocation finished. This is meant for uniform-rate values (the M4
//! `hoist` pass): they do not depend on the pixel, so the caller runs it on the module given to
//! `hoist` (the ids are that module's) at any tiny size (`--width 2 --height 2`), with the
//! scenario's uniforms and samplers, and feeds the values back as `--uniform h_<id>=[...]`.
//! A pixel-rate id is reported too, with its value at pixel (0, 0). An id that was never
//! executed at that pixel (a branch not taken), or that is not a scalar/vector/matrix value,
//! is an error naming the id.

use super::{EvalConfig, Invocation, Program, Value};
use crate::lift::Lifted;
use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;

/// Evaluates the quad at (0, 0) and returns the per-component values (as f64) of `ids` from
/// lane 0. Integers and booleans are converted to float.
pub fn dump_first_quad(lifted: &Lifted, cfg: &EvalConfig, ids: &[u32]) -> Result<BTreeMap<u32, Vec<f64>>> {
    let prog = Program::new(lifted, cfg)?;
    let mut lanes = [Invocation::new(&prog), Invocation::new(&prog), Invocation::new(&prog), Invocation::new(&prog)];
    let mut dead = 0;
    super::run_quad(&mut lanes, 0, 0, &mut dead).with_context(|| format!("{}: at pixel (0, 0)", prog.label))?;
    let mut out = BTreeMap::new();
    for &id in ids {
        if id as usize >= lanes[0].vals.len() {
            bail!("{}: --dump-ids {id}: result id out of range (bound {})", cfg.label, lifted.bound);
        }
        let mut comps = Vec::new();
        leaves(&lanes[0].vals[id as usize], &mut comps)
            .with_context(|| format!("{}: --dump-ids {id}: no value at pixel (0, 0) (not executed there, or not a numeric value)", cfg.label))?;
        out.insert(id, comps);
    }
    Ok(out)
}

fn leaves(v: &Value, out: &mut Vec<f64>) -> Result<()> {
    match v {
        Value::F(x) => out.push(*x),
        Value::I32(x) => out.push(*x as f64),
        Value::U32(x) => out.push(*x as f64),
        Value::Bool(b) => out.push(*b as u32 as f64),
        Value::V(xs) => {
            for x in xs {
                leaves(x, out)?;
            }
        }
        other => bail!("value is {}", other.describe()),
    }
    Ok(())
}

/// `{"57": [1.0, 2.0, 3.0], ...}` as written to `--dump`.
pub fn dump_json(d: &BTreeMap<u32, Vec<f64>>) -> serde_json::Value {
    let mut m = serde_json::Map::new();
    for (id, comps) in d {
        m.insert(id.to_string(), serde_json::Value::Array(comps.iter().map(|x| serde_json::json!(x)).collect()));
    }
    serde_json::Value::Object(m)
}
