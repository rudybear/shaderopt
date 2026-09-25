//! `hoist`: uniform-rate hoisting to the CPU (M4, `shader-ir hoist`).
//!
//! A *hoistable* value is an SSA result of rate `uniform` (not `const`; see
//! [`crate::analysis::rate`]) whose whole subtree is pure and CPU-computable: arithmetic,
//! conversions, comparisons, selects, composites, `GLSL.std.450` math, constants, loads of
//! uniform-block members, and loads of `Function`/`Private` variables with exactly one reaching
//! definition (reaching-definitions dataflow over the CFG per variable, every use of which is a
//! plain `OpStore`/`OpLoad`; the initial value counts as a definition, so a load that may see
//! the uninitialized variable or a loop-carried store is not forwarded). This is how glslang
//! `-V` routes locals, including the per-iteration copies left by `unroll`. Function
//! parameters, phis, calls, image ops and derivatives end the subtree (not hoistable).
//!
//! A hoistable value is *maximal* when it has a consumer that is not absorbed into a larger
//! hoistable computation: a pixel-rate instruction, a phi, a branch, a store to an output or
//! to a variable some load of which cannot be forwarded, or a uniform value of a type that
//! cannot become a member (bool, matrix) with such a consumer. Every maximal value of type
//! `float`/`vec2`/`vec3`/`vec4`/`int` whose subtree holds at least `min_ops` arithmetic or
//! ext-inst instructions (composites and copies are not counted) is replaced by an
//! `OpAccessChain` + `OpLoad` of a new member appended to the shader's `Block`-decorated
//! `Uniform` (else `PushConstant`) block, std140 offset and alignment (float/int 4, vec2 8,
//! vec3/vec4 16), named `h_<id>` by `OpMemberName`. Existing members are untouched. Without a
//! block nothing is edited and the report says so. Loads of variables that forward to the value
//! are redirected too, so the subtree (and the variables that only feed stores into each other
//! afterwards, removed here) is dead code for `dce`.
//!
//! Values under a branch are hoisted too (computing them unconditionally on the CPU is
//! harmless; the branch stays and reads the member). The CPU values come from the interpreter:
//! `eval --dump-ids <source ids> --dump dump.json` on the module given to `hoist` (see
//! [`crate::interp::dump`]); `plan.json` lists, per member, the type, the source id, a textual
//! reconstruction of the expression and the uniform members it depends on.

use super::cfg::Cfg;
use super::consts::{self, CV};
use super::{fresh_id, glsl_op, id_op, inst_at, is_pure, pointer_root, real_uses, remove_defs, replace_uses, Class, EditOp};
use crate::analysis::rate::{self, Rate, Rates};
use crate::analysis::Body;
use crate::lift::{Lifted, Site, Type};
use anyhow::{anyhow, Result};
use rspirv::dr::{Instruction, Operand};
use spirv::{Decoration, Op, StorageClass};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::rc::Rc;

/// One entry of `plan.json`.
#[derive(Clone, Debug)]
pub struct PlanEntry {
    /// `h_<id>`.
    pub member: String,
    /// GLSL type name: `float`, `vec2`, `vec3`, `vec4`, `int`.
    pub ty: String,
    /// The result id (in the input module) whose value the member carries.
    pub source_id: u32,
    /// Best-effort textual reconstruction of the subtree with member names and constants.
    pub expr: String,
    /// Uniform block members the value depends on.
    pub depends_on: Vec<String>,
    /// Arithmetic/ext-inst instructions in the subtree.
    pub ops: usize,
    /// std140 offset of the new member.
    pub offset: u32,
}

impl PlanEntry {
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "member": self.member,
            "type": self.ty,
            "source_id": self.source_id,
            "expr": self.expr,
            "depends_on": self.depends_on,
            "ops": self.ops,
            "offset": self.offset,
        })
    }
}

#[derive(Clone, Debug, Default)]
pub struct HoistReport {
    pub entries: Vec<PlanEntry>,
    /// Name of the uniform block variable the members were appended to; `None` = no block.
    pub block: Option<String>,
    /// Maximal hoistable values found (equal to `entries.len()` when there is a block).
    pub candidates: usize,
    pub bytes_added: u32,
    /// Function variables removed because after the rewrite they only fed stores into each other.
    pub dead_variables: usize,
}

/// Runs the pass. `min_ops` is the minimum number of arithmetic/ext-inst instructions in a
/// hoisted subtree. Returns the plan; `ops` receives one `hoist`/`exact` record per member
/// (and one per removed dead variable).
pub fn run(l: &mut Lifted, ops: &mut Vec<EditOp>, min_ops: usize) -> Result<HoistReport> {
    let block = find_block(l);
    let hoists = {
        let body = Body::new(l);
        let rates = rate::analyze(&body)?;
        select(l, &body, &rates, min_ops)
    };
    let mut report = HoistReport { candidates: hoists.len(), ..Default::default() };
    let Some((block_var, st, storage)) = block else {
        return Ok(report);
    };
    let block_name = l.name(block_var).map(String::from).unwrap_or_else(|| format!("%{block_var}"));
    let struct_name = l.name(st).map(String::from).unwrap_or_else(|| block_name.clone());
    report.block = Some(block_name);
    if hoists.is_empty() {
        return Ok(report);
    }

    // Layout bookkeeping for the block.
    let n_members = match l.types.get(&st) {
        Some(Type::Struct { members }) => members.len() as u32,
        _ => return Err(anyhow!("uniform block %{st} is not a struct")),
    };
    let mut end = block_end(l, st);
    let end0 = end;
    let int_ty = int32_type(l);
    let mut next_member = n_members;

    // Members in source order: struct member, its Offset and its name.
    let Some(Site::Global(si)) = l.defs.get(&st).copied() else {
        return Err(anyhow!("uniform block struct %{st} has no definition site"));
    };
    let mut members: Vec<(u32, u32, String)> = Vec::new(); // (index, offset, name) per hoist
    for h in &hoists {
        let (size, align) = std140_new(l, h.ty).expect("hoist type was checked");
        let offset = align_up(end, align);
        end = offset + size;
        let m = next_member;
        next_member += 1;
        let member = format!("h_{}", h.root);
        l.module.types_global_values[si].operands.push(Operand::IdRef(h.ty));
        l.module.annotations.push(Instruction::new(
            Op::MemberDecorate,
            None,
            None,
            vec![Operand::IdRef(st), Operand::LiteralBit32(m), Operand::Decoration(Decoration::Offset), Operand::LiteralBit32(offset)],
        ));
        l.module.debug_names.push(Instruction::new(
            Op::MemberName,
            None,
            None,
            vec![Operand::IdRef(st), Operand::LiteralBit32(m), Operand::LiteralString(member.clone())],
        ));
        members.push((m, offset, member));
    }
    // A member type must be declared before the struct that uses it. Hoisted value types (e.g. %v3float) can sit
    // after the block struct in glslang output, so move such declarations (and their component scalar) ahead of it.
    {
        let mut si_now = si;
        let tys: Vec<u32> = hoists.iter().map(|h| h.ty).collect();
        for ty in tys {
            let mut chain = vec![ty];
            if let Some(Site::Global(ti)) = l.defs.get(&ty).copied() {
                if l.module.types_global_values[ti].class.opcode == Op::TypeVector {
                    if let Some(Operand::IdRef(comp)) = l.module.types_global_values[ti].operands.first() {
                        chain.insert(0, *comp);
                    }
                }
            }
            for t in chain {
                let Some(pos) = l.module.types_global_values.iter().position(|i| i.result_id == Some(t)) else { continue };
                if pos > si_now {
                    let inst = l.module.types_global_values.remove(pos);
                    l.module.types_global_values.insert(si_now, inst);
                    si_now += 1;
                }
            }
        }
        l.reanalyze();
    }
    // Apply in descending layout order so that insertions never shift a pending site.
    let mut order: Vec<usize> = (0..hoists.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(hoists[i].site));
    let mut entries: Vec<(usize, PlanEntry)> = Vec::new();
    let mut new_ops: Vec<EditOp> = Vec::new();
    for i in order {
        let h = &hoists[i];
        let (m, offset, member) = members[i].clone();
        // The load.
        let ptr_ty = pointer_type(l, storage, h.ty);
        let idx = consts::intern(l, int_ty, &CV::I(m)).ok_or_else(|| anyhow!("cannot intern member index {m}"))?;
        let ac = fresh_id(l);
        let ld = fresh_id(l);
        let (fi, bi, ii) = h.site;
        let insts = &mut l.module.functions[fi].blocks[bi].instructions;
        insts.insert(ii + 1, Instruction::new(Op::AccessChain, Some(ptr_ty), Some(ac), vec![Operand::IdRef(block_var), Operand::IdRef(idx)]));
        insts.insert(ii + 2, Instruction::new(Op::Load, Some(h.ty), Some(ld), vec![Operand::IdRef(ac)]));
        replace_uses(l, h.root, ld);
        for &a in &h.aliases {
            replace_uses(l, a, ld);
        }
        let ty_name = hoist_type_name(l, h.ty).unwrap_or("?");
        new_ops.push(EditOp {
            pass: "hoist",
            class: Class::Exact,
            target: h.root,
            replaced_by: Some(ld),
            detail: format!("hoisted {} instructions into {struct_name}.{member} ({ty_name})", h.ops),
        });
        entries.push((
            i,
            PlanEntry {
                member,
                ty: ty_name.to_string(),
                source_id: h.root,
                expr: h.expr.clone(),
                depends_on: h.deps.clone(),
                ops: h.ops,
                offset,
            },
        ));
    }
    // Report in source order.
    entries.sort_by_key(|(i, _)| *i);
    new_ops.reverse();
    ops.extend(new_ops);
    report.entries = entries.into_iter().map(|(_, e)| e).collect();
    report.bytes_added = end - end0;
    l.reanalyze()?;
    report.dead_variables = remove_dead_variables(l, ops);
    Ok(report)
}

// ---- block layout ----------------------------------------------------------------------------

/// The `Block`-decorated `Uniform` variable (else `PushConstant`): `(variable, struct, storage)`.
fn find_block(l: &Lifted) -> Option<(u32, u32, StorageClass)> {
    for sc in [StorageClass::Uniform, StorageClass::PushConstant] {
        if let Some(v) = l.variables.iter().find(|v| v.storage == sc && v.block && matches!(l.types.get(&v.pointee), Some(Type::Struct { .. }))) {
            return Some((v.id, v.pointee, sc));
        }
    }
    None
}

/// `(size, alignment)` of a new std140 member of a hoistable type.
fn std140_new(l: &Lifted, ty: u32) -> Option<(u32, u32)> {
    match l.types.get(&ty)? {
        Type::Float { width: 32 } | Type::Int { width: 32, signed: true } => Some((4, 4)),
        Type::Vector { elem, count } if matches!(l.types.get(elem), Some(Type::Float { width: 32 })) => match count {
            2 => Some((8, 8)),
            3 => Some((12, 16)),
            4 => Some((16, 16)),
            _ => None,
        },
        _ => None,
    }
}

/// GLSL spelling of a hoistable type, `None` for any other type.
pub fn hoist_type_name(l: &Lifted, ty: u32) -> Option<&'static str> {
    match l.types.get(&ty)? {
        Type::Float { width: 32 } => Some("float"),
        Type::Int { width: 32, signed: true } => Some("int"),
        Type::Vector { elem, count } if matches!(l.types.get(elem), Some(Type::Float { width: 32 })) => match count {
            2 => Some("vec2"),
            3 => Some("vec3"),
            4 => Some("vec4"),
            _ => None,
        },
        _ => None,
    }
}

fn align_up(x: u32, a: u32) -> u32 {
    (x + a - 1) / a * a
}

/// Best-effort std140 size of an existing member type (matrix/array strides from decorations).
fn std140_size(l: &Lifted, ty: u32, member: Option<(u32, u32)>) -> u32 {
    match l.types.get(&ty) {
        Some(Type::Bool) | Some(Type::Int { .. }) | Some(Type::Float { width: 32 }) | Some(Type::Float { width: 16 }) => 4,
        Some(Type::Float { .. }) => 8,
        Some(Type::Vector { elem, count }) => std140_size(l, *elem, None) * count,
        Some(Type::Matrix { column, columns }) => {
            let stride = member
                .and_then(|(st, m)| l.member_decoration_u32(st, m, Decoration::MatrixStride))
                .unwrap_or_else(|| align_up(std140_size(l, *column, None), 16));
            let row_major = member.map_or(false, |(st, m)| {
                l.member_decorations.get(&(st, m)).map_or(false, |d| d.iter().any(|x| x.decoration == Decoration::RowMajor))
            });
            let rows = match l.types.get(column) {
                Some(Type::Vector { count, .. }) => *count,
                _ => *columns,
            };
            stride * if row_major { rows } else { *columns }
        }
        Some(Type::Array { elem, len }) => {
            let n = l.array_len(*len).unwrap_or(1);
            let stride = l.decoration_u32(ty, Decoration::ArrayStride).unwrap_or_else(|| align_up(std140_size(l, *elem, None), 16));
            stride * n
        }
        Some(Type::Struct { .. }) => align_up(block_end(l, ty), 16),
        _ => 16,
    }
}

/// Offset just past the last byte of any member of struct `st`.
fn block_end(l: &Lifted, st: u32) -> u32 {
    let Some(Type::Struct { members }) = l.types.get(&st) else { return 0 };
    let mut end = 0;
    let mut running = 0;
    for (m, mty) in members.iter().enumerate() {
        let off = l.member_decoration_u32(st, m as u32, Decoration::Offset).unwrap_or(running);
        let size = std140_size(l, *mty, Some((st, m as u32)));
        running = off + size;
        end = end.max(running);
    }
    end
}

fn pointer_type(l: &mut Lifted, storage: StorageClass, pointee: u32) -> u32 {
    if let Some((id, _)) = l.types.iter().find(|(_, t)| matches!(t, Type::Pointer { storage: s, pointee: p } if *s == storage && *p == pointee)) {
        return *id;
    }
    let id = fresh_id(l);
    l.module.types_global_values.push(Instruction::new(Op::TypePointer, None, Some(id), vec![Operand::StorageClass(storage), Operand::IdRef(pointee)]));
    l.types.insert(id, Type::Pointer { storage, pointee });
    l.defs.insert(id, Site::Global(l.module.types_global_values.len() - 1));
    id
}

fn int32_type(l: &mut Lifted) -> u32 {
    if let Some((id, _)) = l.types.iter().find(|(_, t)| matches!(t, Type::Int { width: 32, signed: true })) {
        return *id;
    }
    let id = fresh_id(l);
    l.module.types_global_values.push(Instruction::new(Op::TypeInt, None, Some(id), vec![Operand::LiteralBit32(32), Operand::LiteralBit32(1)]));
    l.types.insert(id, Type::Int { width: 32, signed: true });
    l.defs.insert(id, Site::Global(l.module.types_global_values.len() - 1));
    id
}

// ---- selection -------------------------------------------------------------------------------

/// A maximal hoistable value.
struct Hoist {
    root: u32,
    ty: u32,
    /// Definition site of `root`: the load is inserted right after it.
    site: (usize, usize, usize),
    /// Loads of Function/Private variables that forward to `root`.
    aliases: Vec<u32>,
    ops: usize,
    expr: String,
    deps: Vec<String>,
}

/// The pure subtree of a value.
struct Sub {
    /// Counted instruction ids (arithmetic, conversions, comparisons, selects, ext-inst).
    nodes: BTreeSet<u32>,
    deps: BTreeSet<String>,
    expr: String,
}

const EXPR_CAP: usize = 4000;

struct Sel<'a> {
    l: &'a Lifted,
    body: &'a Body<'a>,
    rates: &'a Rates,
    /// Load id -> the value of its unique reaching store.
    resolve: HashMap<u32, u32>,
    /// Variables every load of which is forwardable (a store to them is transparent).
    transparent: HashSet<u32>,
    /// Final value -> loads forwarding to it (through chains of variables).
    aliases: HashMap<u32, Vec<u32>>,
    memo: HashMap<u32, Option<Rc<Sub>>>,
    visiting: HashSet<u32>,
    absorb_memo: HashMap<u32, bool>,
    absorb_visiting: HashSet<u32>,
}

fn select(l: &Lifted, body: &Body<'_>, rates: &Rates, min_ops: usize) -> Vec<Hoist> {
    let mut s = Sel {
        l,
        body,
        rates,
        resolve: HashMap::new(),
        transparent: HashSet::new(),
        aliases: HashMap::new(),
        memo: HashMap::new(),
        visiting: HashSet::new(),
        absorb_memo: HashMap::new(),
        absorb_visiting: HashSet::new(),
    };
    s.build_forwarding();
    let mut out = Vec::new();
    for bi in &body.insts {
        let inst = bi.inst;
        let Some(v) = inst.result_id else { continue };
        let Some(block) = bi.block else { continue };
        if inst.class.opcode == Op::Load || l.constants.contains_key(&v) {
            continue;
        }
        if rates.rate_of_id(v) != Rate::Uniform {
            continue;
        }
        let Some(ty) = inst.result_type else { continue };
        if hoist_type_name(l, ty).is_none() {
            continue;
        }
        let Some(sub) = s.info(v) else { continue };
        if sub.nodes.len() < min_ops {
            continue;
        }
        if !s.is_maximal(v) {
            continue;
        }
        let Some(Site::Inst(fi, b2, ii)) = l.defs.get(&v).copied() else { continue };
        debug_assert_eq!(b2, block);
        let mut expr = sub.expr.clone();
        if expr.len() > EXPR_CAP {
            expr.truncate(EXPR_CAP);
            expr.push_str(" ...");
        }
        out.push(Hoist {
            root: v,
            ty,
            site: (fi, b2, ii),
            aliases: s.aliases.get(&v).cloned().unwrap_or_default(),
            ops: sub.nodes.len(),
            expr,
            deps: sub.deps.iter().cloned().collect(),
        });
    }
    out
}

impl<'a> Sel<'a> {
    /// Store-to-load forwarding for variables whose every use is a plain store or load and
    /// whose every store strictly dominates the load.
    fn build_forwarding(&mut self) {
        let l = self.l;
        let cfgs: Vec<Cfg> = (0..l.module.functions.len()).map(|fi| Cfg::build(l, fi)).collect();
        // Candidate variables: Function variables and Private globals.
        let mut vars: Vec<u32> = Vec::new();
        for f in &l.module.functions {
            for b in &f.blocks {
                for inst in &b.instructions {
                    if inst.class.opcode == Op::Variable {
                        if let Some(id) = inst.result_id {
                            vars.push(id);
                        }
                    }
                }
            }
        }
        vars.extend(l.variables.iter().filter(|v| v.storage == StorageClass::Private).map(|v| v.id));
        for var in vars {
            let mut stores: Vec<(usize, usize, usize, u32)> = Vec::new();
            let mut loads: Vec<(usize, usize, usize, u32)> = Vec::new();
            let mut simple = true;
            for site in real_uses(l, var) {
                let (Site::Inst(fi, bi, ii), Some(inst)) = (*site, inst_at(l, *site)) else {
                    simple = false;
                    break;
                };
                match inst.class.opcode {
                    Op::Store if id_op(inst, 0) == Some(var) && id_op(inst, 1) != Some(var) && inst.operands.len() == 2 => {
                        stores.push((fi, bi, ii, id_op(inst, 1).unwrap()));
                    }
                    Op::Load if id_op(inst, 0) == Some(var) && inst.operands.len() == 1 => {
                        loads.push((fi, bi, ii, inst.result_id.unwrap()));
                    }
                    _ => {
                        simple = false;
                        break;
                    }
                }
            }
            if !simple || stores.is_empty() {
                continue;
            }
            let fi0 = stores[0].0;
            if stores.iter().any(|s| s.0 != fi0) || loads.iter().any(|x| x.0 != fi0) {
                continue;
            }
            let cfg = &cfgs[fi0];
            // Reaching definitions over the function's CFG: def `n` (= stores.len()) is the
            // initial (zero / initializer) value at the entry; a block with a store kills
            // every incoming definition.
            let nb = cfg.labels.len();
            let init = stores.len();
            let mut gen: Vec<Option<usize>> = vec![None; nb];
            for (k, &(_, sb, si, _)) in stores.iter().enumerate() {
                if gen[sb].map_or(true, |g| stores[g].2 < si) {
                    gen[sb] = Some(k);
                }
            }
            let mut inn: Vec<Vec<usize>> = vec![Vec::new(); nb];
            let mut out: Vec<Vec<usize>> = vec![Vec::new(); nb];
            if nb > 0 {
                inn[0] = vec![init];
            }
            loop {
                let mut changed = false;
                for b in 0..nb {
                    if !cfg.reachable(b) {
                        continue;
                    }
                    let mut i: Vec<usize> = if b == 0 { vec![init] } else { Vec::new() };
                    for &p in &cfg.pred[b] {
                        for &d in &out[p] {
                            if !i.contains(&d) {
                                i.push(d);
                            }
                        }
                    }
                    i.sort_unstable();
                    let o: Vec<usize> = match gen[b] {
                        Some(g) => vec![g],
                        None => i.clone(),
                    };
                    if i != inn[b] || o != out[b] {
                        inn[b] = i;
                        out[b] = o;
                        changed = true;
                    }
                }
                if !changed {
                    break;
                }
            }
            let mut all_forwardable = true;
            for &(_, lb, li, lid) in &loads {
                // The last store before the load in its own block, else the block's entry set.
                let local = stores.iter().enumerate().filter(|(_, s)| s.1 == lb && s.2 < li).max_by_key(|(_, s)| s.2).map(|(k, _)| k);
                let reaching: Vec<usize> = match local {
                    Some(k) => vec![k],
                    None => inn[lb].clone(),
                };
                if reaching.len() == 1 && reaching[0] != init {
                    self.resolve.insert(lid, stores[reaching[0]].3);
                } else {
                    all_forwardable = false;
                }
            }
            if all_forwardable {
                self.transparent.insert(var);
            }
        }
        // Alias lists: follow chains of forwarded loads to the final value.
        let loads: Vec<u32> = self.resolve.keys().copied().collect();
        for lid in loads {
            let mut cur = lid;
            let mut fin = None;
            for _ in 0..64 {
                match self.resolve.get(&cur) {
                    Some(&v) => cur = v,
                    None => {
                        fin = Some(cur);
                        break;
                    }
                }
            }
            if let Some(f) = fin {
                self.aliases.entry(f).or_default().push(lid);
            }
        }
    }

    fn info(&mut self, v: u32) -> Option<Rc<Sub>> {
        if let Some(m) = self.memo.get(&v) {
            return m.clone();
        }
        if !self.visiting.insert(v) {
            return None;
        }
        let r = self.compute(v);
        self.visiting.remove(&v);
        self.memo.insert(v, r.clone());
        r
    }

    fn compute(&mut self, v: u32) -> Option<Rc<Sub>> {
        let l = self.l;
        if let Some(c) = l.constants.get(&v) {
            if c.spec {
                return None;
            }
            let cv = consts::read(l, v)?;
            return Some(Rc::new(Sub { nodes: BTreeSet::new(), deps: BTreeSet::new(), expr: fmt_const(l, c.ty, &cv) }));
        }
        let d = self.body.def(v)?;
        let inst = d.inst;
        let ids = Body::id_operands(inst);
        if inst.class.opcode == Op::Load {
            if inst.operands.len() != 1 {
                return None;
            }
            let ptr = ids[0];
            let (root, storage) = pointer_root(l, ptr)?;
            return match storage {
                StorageClass::Uniform | StorageClass::PushConstant => {
                    let (top, path) = self.member_path(root, ptr)?;
                    let mut deps = BTreeSet::new();
                    deps.insert(top);
                    Some(Rc::new(Sub { nodes: BTreeSet::new(), deps, expr: path }))
                }
                StorageClass::Function | StorageClass::Private => {
                    let val = *self.resolve.get(&v)?;
                    self.info(val)
                }
                _ => None,
            };
        }
        let shape = classify(l, inst)?;
        let mut subs: Vec<Rc<Sub>> = Vec::with_capacity(ids.len());
        for o in &ids {
            subs.push(self.info(*o)?);
        }
        let mut nodes = BTreeSet::new();
        let mut deps = BTreeSet::new();
        for s in &subs {
            nodes.extend(s.nodes.iter().copied());
            deps.extend(s.deps.iter().cloned());
        }
        if shape.counts {
            nodes.insert(v);
        }
        let args: Vec<&str> = subs.iter().map(|s| s.expr.as_str()).collect();
        let expr = shape.format(l, inst, &args);
        Some(Rc::new(Sub { nodes, deps, expr }))
    }

    /// `(top-level member name, full path)` of a constant-index access chain into a uniform
    /// block variable (`sigma`, `texel.x`, `mat_r.rgb`-style paths are spelled `mat_r[0]`).
    fn member_path(&self, root: u32, ptr: u32) -> Option<(String, String)> {
        let l = self.l;
        let var = l.variables.iter().find(|v| v.id == root)?;
        let var_name = var.name.clone().unwrap_or_else(|| format!("%{root}"));
        if ptr == root {
            return Some((var_name.clone(), var_name));
        }
        // Collect the chain indices from the variable outwards.
        let mut chain: Vec<u32> = Vec::new();
        let mut cur = ptr;
        for _ in 0..64 {
            if cur == root {
                break;
            }
            let inst = super::def_inst(l, cur)?;
            match inst.class.opcode {
                Op::AccessChain | Op::InBoundsAccessChain => {
                    let idx: Vec<u32> = inst.operands.iter().skip(1).filter_map(|o| o.id_ref_any()).collect();
                    for i in idx.iter().rev() {
                        chain.push(*i);
                    }
                    cur = id_op(inst, 0)?;
                }
                Op::CopyObject => cur = id_op(inst, 0)?,
                _ => return None,
            }
        }
        chain.reverse();
        let mut ty = var.pointee;
        let mut path = String::new();
        let mut top: Option<String> = None;
        for (k, c) in chain.iter().enumerate() {
            let i = match consts::read(l, *c)? {
                CV::I(i) => i,
                _ => return None,
            };
            match l.types.get(&ty)? {
                Type::Struct { members } => {
                    let name = l.member_name(ty, i).map(String::from).unwrap_or_else(|| format!("member{i}"));
                    if k == 0 {
                        top = Some(name.clone());
                        path = name;
                    } else {
                        path.push('.');
                        path.push_str(&name);
                    }
                    ty = *members.get(i as usize)?;
                }
                Type::Vector { elem, .. } => {
                    path.push('.');
                    path.push(['x', 'y', 'z', 'w'].get(i as usize).copied().unwrap_or('?'));
                    ty = *elem;
                }
                Type::Matrix { column, .. } => {
                    path.push_str(&format!("[{i}]"));
                    ty = *column;
                }
                Type::Array { elem, .. } => {
                    path.push_str(&format!("[{i}]"));
                    ty = *elem;
                }
                _ => return None,
            }
        }
        let top = top.unwrap_or_else(|| var_name.clone());
        Some((top, path))
    }

    /// True when some consumer of `v` (or of a load forwarding to `v`) is not absorbed into a
    /// larger hoistable computation.
    fn is_maximal(&mut self, v: u32) -> bool {
        let consumers = self.consumers(v);
        consumers.into_iter().any(|site| !self.absorbs(site))
    }

    fn consumers(&self, v: u32) -> Vec<Site> {
        let mut out: Vec<Site> = real_uses(self.l, v).copied().collect();
        if let Some(al) = self.aliases.get(&v) {
            for a in al {
                out.extend(real_uses(self.l, *a).copied());
            }
        }
        out
    }

    /// Does the consumer at `site` absorb its hoistable operand into a larger hoistable value?
    fn absorbs(&mut self, site: Site) -> bool {
        let l = self.l;
        let Some(inst) = inst_at(l, site) else { return false };
        if !matches!(site, Site::Inst(..)) {
            return false;
        }
        if inst.class.opcode == Op::Store {
            // Transparent when the stored-to variable forwards every load.
            return id_op(inst, 0).map_or(false, |p| self.transparent.contains(&p));
        }
        let Some(r) = inst.result_id else { return false };
        self.absorbed_value(r)
    }

    /// A value absorbs its operands when it is hoistable itself and either has a hoistable
    /// type (it is hoisted or absorbed in turn) or every one of its consumers absorbs it.
    fn absorbed_value(&mut self, r: u32) -> bool {
        if let Some(&b) = self.absorb_memo.get(&r) {
            return b;
        }
        if !self.absorb_visiting.insert(r) {
            return false;
        }
        let l = self.l;
        let res = if self.rates.rate_of_id(r) != Rate::Uniform || self.info(r).is_none() {
            false
        } else if l.result_types.get(&r).map_or(false, |t| hoist_type_name(l, *t).is_some()) {
            true
        } else {
            let cs = self.consumers(r);
            !cs.is_empty() && cs.into_iter().all(|c| self.absorbs(c))
        };
        self.absorb_visiting.remove(&r);
        self.absorb_memo.insert(r, res);
        res
    }
}

// ---- instruction shapes ------------------------------------------------------------------------

enum Shape {
    Binary(&'static str),
    Unary(&'static str),
    Call(String),
    Select,
    Construct,
    Extract,
    Shuffle,
    Copy,
}

struct Classified {
    shape: Shape,
    counts: bool,
}

impl Classified {
    fn format(&self, l: &Lifted, inst: &Instruction, a: &[&str]) -> String {
        let arg = |i: usize| a.get(i).copied().unwrap_or("?");
        match &self.shape {
            Shape::Binary(op) => format!("({} {op} {})", arg(0), arg(1)),
            Shape::Unary(op) => format!("{op}{}", arg(0)),
            Shape::Call(name) => format!("{name}({})", a.join(", ")),
            Shape::Select => format!("({} ? {} : {})", arg(0), arg(1), arg(2)),
            Shape::Construct => {
                let ty = inst.result_type.map(|t| l.type_name(t)).unwrap_or_else(|| "composite".into());
                format!("{ty}({})", a.join(", "))
            }
            Shape::Extract => {
                let lits: Vec<u32> = lits(inst, 1);
                let src_ty = id_op(inst, 0).and_then(|s| l.result_types.get(&s).or_else(|| l.constants.get(&s).map(|c| &c.ty)));
                if lits.len() == 1 && matches!(src_ty.and_then(|t| l.types.get(t)), Some(Type::Vector { .. })) {
                    format!("{}.{}", arg(0), swizzle(&lits))
                } else {
                    format!("{}{}", arg(0), lits.iter().map(|i| format!("[{i}]")).collect::<String>())
                }
            }
            Shape::Shuffle => {
                let lits: Vec<u32> = lits(inst, 2);
                let n0 = id_op(inst, 0)
                    .and_then(|s| l.result_types.get(&s).or_else(|| l.constants.get(&s).map(|c| &c.ty)))
                    .and_then(|t| match l.types.get(t) {
                        Some(Type::Vector { count, .. }) => Some(*count),
                        _ => None,
                    })
                    .unwrap_or(0);
                if lits.iter().all(|i| *i < n0) {
                    format!("{}.{}", arg(0), swizzle(&lits))
                } else if id_op(inst, 0) == id_op(inst, 1) {
                    format!("{}.{}", arg(0), swizzle(&lits.iter().map(|i| i % n0.max(1)).collect::<Vec<_>>()))
                } else {
                    format!("shuffle({}, {}, [{}])", arg(0), arg(1), lits.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(","))
                }
            }
            Shape::Copy => arg(0).to_string(),
        }
    }
}

fn lits(inst: &Instruction, skip: usize) -> Vec<u32> {
    inst.operands
        .iter()
        .skip(skip)
        .filter_map(|o| match o {
            Operand::LiteralBit32(v) => Some(*v),
            _ => None,
        })
        .collect()
}

fn swizzle(idx: &[u32]) -> String {
    idx.iter().map(|i| ['x', 'y', 'z', 'w'].get(*i as usize).copied().unwrap_or('?')).collect()
}

/// Whether an instruction may be part of a hoisted subtree, how to spell it, and whether it
/// counts as an arithmetic/ext-inst op. `None` = ends the subtree (not hoistable).
fn classify(l: &Lifted, inst: &Instruction) -> Option<Classified> {
    use Op::*;
    let bin = |s: &'static str| Some(Classified { shape: Shape::Binary(s), counts: true });
    let un = |s: &'static str| Some(Classified { shape: Shape::Unary(s), counts: true });
    let call = |s: &str| Some(Classified { shape: Shape::Call(s.to_string()), counts: true });
    match inst.class.opcode {
        FAdd | IAdd => bin("+"),
        FSub | ISub => bin("-"),
        FMul | IMul | VectorTimesScalar | MatrixTimesScalar | VectorTimesMatrix | MatrixTimesVector | MatrixTimesMatrix => bin("*"),
        FDiv | SDiv | UDiv => bin("/"),
        FRem | SRem | UMod | SMod | FMod => bin("%"),
        FNegate | SNegate => un("-"),
        LogicalNot | Not => un("!"),
        Dot => call("dot"),
        OuterProduct => call("outerProduct"),
        Transpose => call("transpose"),
        FOrdLessThan | FUnordLessThan | SLessThan | ULessThan => bin("<"),
        FOrdGreaterThan | FUnordGreaterThan | SGreaterThan | UGreaterThan => bin(">"),
        FOrdLessThanEqual | FUnordLessThanEqual | SLessThanEqual | ULessThanEqual => bin("<="),
        FOrdGreaterThanEqual | FUnordGreaterThanEqual | SGreaterThanEqual | UGreaterThanEqual => bin(">="),
        FOrdEqual | FUnordEqual | IEqual | LogicalEqual => bin("=="),
        FOrdNotEqual | FUnordNotEqual | INotEqual | LogicalNotEqual => bin("!="),
        LogicalAnd => bin("&&"),
        LogicalOr => bin("||"),
        BitwiseAnd => bin("&"),
        BitwiseOr => bin("|"),
        BitwiseXor => bin("^"),
        ShiftLeftLogical => bin("<<"),
        ShiftRightLogical | ShiftRightArithmetic => bin(">>"),
        ConvertSToF | ConvertUToF => call("float"),
        ConvertFToS => call("int"),
        ConvertFToU => call("uint"),
        FConvert | SConvert | UConvert | QuantizeToF16 => call("convert"),
        Bitcast => call("bitcast"),
        Any => call("any"),
        All => call("all"),
        IsNan => call("isnan"),
        IsInf => call("isinf"),
        Select => Some(Classified { shape: Shape::Select, counts: true }),
        CompositeConstruct => Some(Classified { shape: Shape::Construct, counts: false }),
        CompositeExtract => Some(Classified { shape: Shape::Extract, counts: false }),
        VectorShuffle => Some(Classified { shape: Shape::Shuffle, counts: false }),
        CopyObject | CopyLogical => Some(Classified { shape: Shape::Copy, counts: false }),
        ExtInst => {
            let g = glsl_op(l, inst)?;
            if matches!(g, spirv::GLOp::InterpolateAtCentroid | spirv::GLOp::InterpolateAtSample | spirv::GLOp::InterpolateAtOffset) {
                return None;
            }
            call(&glsl_name(g))
        }
        _ => None,
    }
}

/// GLSL-ish spelling of a GLSL.std.450 op: `FMax -> max`, `Exp -> exp`, `InverseSqrt -> inversesqrt`.
pub fn glsl_name(g: spirv::GLOp) -> String {
    let s = format!("{g:?}");
    let s = match s.as_bytes() {
        [b'F' | b'S' | b'U' | b'N', c, ..] if c.is_ascii_uppercase() && s.len() > 2 => s[1..].to_string(),
        _ => s,
    };
    s.to_lowercase()
}

fn fmt_const(l: &Lifted, ty: u32, cv: &CV) -> String {
    match cv {
        CV::Comp(_) => format!("{}{}", l.type_name(ty), consts::fmt_cv(cv)),
        _ => consts::fmt_cv(cv),
    }
}

// ---- dead variables ------------------------------------------------------------------------------

/// Removes Function variables whose loads, transitively through pure instructions, only feed
/// stores back into such variables (the accumulator chains left behind by hoisting). The
/// variables and their stores go; the loads and arithmetic become unused for `dce`.
pub fn remove_dead_variables(l: &mut Lifted, ops: &mut Vec<EditOp>) -> usize {
    let mut cands: HashSet<u32> = HashSet::new();
    let mut loads_of: HashMap<u32, Vec<u32>> = HashMap::new();
    for f in &l.module.functions {
        for b in &f.blocks {
            for inst in &b.instructions {
                if inst.class.opcode != Op::Variable {
                    continue;
                }
                let var = inst.result_id.unwrap();
                let mut simple = true;
                let mut loads = Vec::new();
                for site in real_uses(l, var) {
                    let (Site::Inst(..), Some(u)) = (*site, inst_at(l, *site)) else {
                        simple = false;
                        break;
                    };
                    match u.class.opcode {
                        Op::Store if id_op(u, 0) == Some(var) && id_op(u, 1) != Some(var) => {}
                        Op::Load if id_op(u, 0) == Some(var) && u.operands.len() == 1 => loads.push(u.result_id.unwrap()),
                        _ => {
                            simple = false;
                            break;
                        }
                    }
                }
                if simple && !loads.is_empty() {
                    cands.insert(var);
                    loads_of.insert(var, loads);
                }
            }
        }
    }
    loop {
        let mut drop: Vec<u32> = Vec::new();
        for &var in &cands {
            let mut ok = true;
            let mut stack: Vec<u32> = loads_of[&var].clone();
            let mut seen: HashSet<u32> = HashSet::new();
            'outer: while let Some(x) = stack.pop() {
                if !seen.insert(x) {
                    continue;
                }
                for site in real_uses(l, x) {
                    let (Site::Inst(..), Some(u)) = (*site, inst_at(l, *site)) else {
                        ok = false;
                        break 'outer;
                    };
                    match u.class.opcode {
                        Op::Store => {
                            let to_cand = id_op(u, 1) == Some(x) && id_op(u, 0) != Some(x) && id_op(u, 0).map_or(false, |p| cands.contains(&p));
                            if !to_cand {
                                ok = false;
                                break 'outer;
                            }
                        }
                        Op::Variable => {
                            ok = false;
                            break 'outer;
                        }
                        _ if is_pure(l, u) => stack.push(u.result_id.unwrap()),
                        _ => {
                            ok = false;
                            break 'outer;
                        }
                    }
                }
            }
            if !ok {
                drop.push(var);
            }
        }
        if drop.is_empty() {
            break;
        }
        for v in drop {
            cands.remove(&v);
        }
    }
    if cands.is_empty() {
        return 0;
    }
    let mut n_stores: HashMap<u32, usize> = HashMap::new();
    for f in &mut l.module.functions {
        for b in &mut f.blocks {
            b.instructions.retain(|i| {
                if i.class.opcode == Op::Store {
                    if let Some(p) = id_op(i, 0) {
                        if cands.contains(&p) {
                            *n_stores.entry(p).or_default() += 1;
                            return false;
                        }
                    }
                }
                true
            });
        }
    }
    let mut sorted: Vec<u32> = cands.iter().copied().collect();
    sorted.sort();
    for var in &sorted {
        ops.push(EditOp {
            pass: "hoist",
            class: Class::Exact,
            target: *var,
            replaced_by: None,
            detail: format!(
                "OpVariable {} only fed stores into dead variables after hoisting; removed {} store(s)",
                l.name(*var).map(|s| format!("\"{s}\"")).unwrap_or_default(),
                n_stores.get(var).copied().unwrap_or(0)
            ),
        });
    }
    remove_defs(l, &cands);
    l.reanalyze().expect("reanalyze after hoist dead-variable removal");
    sorted.len()
}
