//! `eval --dump-ids 57,58 --dump dump.json`: the values of listed result ids at one pixel.
//!
//! Quads are evaluated from (0, 0) onward, row by row over the configured image, until every
//! listed id has a value; the values come from the first lane that executed the id. This is
//! meant for uniform-rate values (the M4 `hoist` pass): they do not depend on the pixel, so
//! the caller runs it on the module given to `hoist` (the ids are that module's) at any tiny
//! size (`--width 2 --height 2` unless pixel (0, 0) is discarded or the value sits under a
//! pixel-rate branch, when a larger size finds a pixel that computes it), with the scenario's
//! uniforms and samplers, and feeds the values back as `--uniform h_<id>=[...]`. A pixel-rate
//! id is reported too, with its value at the first pixel that computed it. An id that no
//! evaluated pixel executed (a uniform-rate branch not taken in this scenario: the hoisted
//! member is then never read) is zero-filled and listed under `"_never_executed"`; an id that
//! is not a scalar/vector/matrix value is an error naming the id.

use super::{EvalConfig, Invocation, Program, Value};
use crate::lift::{Lifted, Type};
use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default)]
pub struct Dump {
    /// Per id: its components as f64 (integers and booleans converted; zeros when never
    /// executed).
    pub values: BTreeMap<u32, Vec<f64>>,
    pub never_executed: Vec<u32>,
    /// Pixel whose lane provided the last value found (for messages).
    pub pixel: (usize, usize),
}

/// Evaluates quads from (0, 0) until every id in `ids` has a value (see the module doc).
pub fn dump_values(lifted: &Lifted, cfg: &EvalConfig, ids: &[u32]) -> Result<Dump> {
    let prog = Program::new(lifted, cfg)?;
    for &id in ids {
        if id as usize >= prog.template_vals.len() {
            bail!("{}: --dump-ids {id}: result id out of range (bound {})", cfg.label, lifted.bound);
        }
        let ty = lifted.result_types.get(&id).copied().or_else(|| lifted.constants.get(&id).map(|c| c.ty));
        if ty.map_or(true, |t| leaf_count(lifted, t).is_none()) {
            bail!("{}: --dump-ids {id}: not a scalar/vector/matrix result id", cfg.label);
        }
    }
    let mut lanes = [Invocation::new(&prog), Invocation::new(&prog), Invocation::new(&prog), Invocation::new(&prog)];
    let mut dead = 0;
    let mut out = Dump::default();
    let (w, h) = (cfg.width.max(1), cfg.height.max(1));
    'quads: for y0 in (0..h).step_by(2) {
        for x0 in (0..w).step_by(2) {
            super::run_quad(&mut lanes, x0, y0, &mut dead).with_context(|| format!("{}: at pixel ({x0}, {y0})", prog.label))?;
            for lane in lanes.iter() {
                for &id in ids {
                    if out.values.contains_key(&id) {
                        continue;
                    }
                    let mut comps = Vec::new();
                    if leaves(&lane.vals[id as usize], &mut comps).is_ok() {
                        out.values.insert(id, comps);
                        out.pixel = (lane.x, lane.y);
                    }
                }
            }
            if ids.iter().all(|id| out.values.contains_key(id)) {
                break 'quads;
            }
        }
    }
    for &id in ids {
        if !out.values.contains_key(&id) {
            let ty = lifted.result_types.get(&id).copied().or_else(|| lifted.constants.get(&id).map(|c| c.ty)).unwrap();
            out.values.insert(id, vec![0.0; leaf_count(lifted, ty).unwrap()]);
            out.never_executed.push(id);
        }
    }
    Ok(out)
}

/// Number of scalar leaves of a scalar/vector/matrix type.
fn leaf_count(l: &Lifted, ty: u32) -> Option<usize> {
    match l.types.get(&ty)? {
        Type::Bool | Type::Int { .. } | Type::Float { .. } => Some(1),
        Type::Vector { elem, count } => Some(leaf_count(l, *elem)? * *count as usize),
        Type::Matrix { column, columns } => Some(leaf_count(l, *column)? * *columns as usize),
        _ => None,
    }
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

/// `{"57": [1.0, 2.0, 3.0], ..., "_never_executed": [54]}` (the last key only when non-empty).
pub fn dump_json(d: &Dump) -> serde_json::Value {
    let mut m = serde_json::Map::new();
    for (id, comps) in &d.values {
        m.insert(id.to_string(), serde_json::Value::Array(comps.iter().map(|x| serde_json::json!(x)).collect()));
    }
    if !d.never_executed.is_empty() {
        m.insert("_never_executed".into(), serde_json::json!(d.never_executed));
    }
    serde_json::Value::Object(m)
}
