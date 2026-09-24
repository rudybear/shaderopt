//! One fragment invocation as a resumable state machine.
//!
//! `Invocation::run` executes instructions until the entry function returns (`Finished`), the
//! invocation is killed (`Discarded`) or a derivative instruction is reached (`Yield`). The quad
//! driver in the parent module then collects the operand value from all four lanes of the quad,
//! computes each lane's derivative and calls `resume`. This is how GPUs evaluate `dFdx`: the
//! difference between the two horizontally adjacent pixels of a 2x2 quad (right minus left) and,
//! for `dFdy`, the two vertically adjacent ones (bottom minus top). Both lanes of a row see the
//! same `dFdx`. "Fine" variants use the requesting lane's own row/column, "coarse" variants use
//! the quad's top row / left column; with 2x2 quads that only differs when lanes are dead.
//! A lane that has already been discarded provides no value: the derivative then falls back to 0
//! and is counted in `EvalOutput::dead_derivatives` (a GPU would give an undefined value).

use super::image::{fetch_2d, sample_2d};
use super::value::{map1, map2, map3, mk_int, Fp, Ptr, Value};
use super::{ext, Kind, Program};
use crate::lift::Type;
use anyhow::{anyhow, bail, Context, Result};
use rspirv::dr::{Instruction, Operand};
use spirv::Op;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DerivKind {
    Dx,
    Dy,
    Fwidth,
}

#[derive(Clone, Debug)]
pub struct DerivReq {
    pub result: u32,
    pub ty: u32,
    pub kind: DerivKind,
    pub fine: bool,
    pub value: Value,
}

#[derive(Clone, Debug)]
pub enum Status {
    Finished,
    Discarded,
    Yield(DerivReq),
}

enum Flow {
    Next,
    Branch(u32),
    Return(Option<Value>),
    Kill,
    Yield(DerivReq),
    Call { func: usize, args: Vec<Value>, ret: Option<u32> },
}

struct Frame {
    func: usize,
    block: usize,
    pc: usize,
    prev_label: u32,
    ret_target: Option<u32>,
}

pub struct Invocation<'a> {
    pub prog: &'a Program<'a>,
    /// SSA values by result id.
    pub vals: Vec<Value>,
    /// Variable contents by variable id.
    pub mem: Vec<Value>,
    stack: Vec<Frame>,
    pub x: usize,
    pub y: usize,
    demoted: bool,
}

impl<'a> Invocation<'a> {
    pub fn new(prog: &'a Program<'a>) -> Self {
        Invocation {
            prog,
            vals: prog.template_vals.clone(),
            mem: prog.template_mem.clone(),
            stack: Vec::new(),
            x: 0,
            y: 0,
            demoted: false,
        }
    }

    /// Prepares the invocation for pixel `(x, y)`.
    pub fn reset(&mut self, x: usize, y: usize) {
        self.vals.clone_from(&self.prog.template_vals);
        self.mem.clone_from(&self.prog.template_mem);
        self.x = x;
        self.y = y;
        self.demoted = false;
        self.stack.clear();
        self.stack.push(Frame { func: self.prog.entry_fn, block: 0, pc: 0, prev_label: 0, ret_target: None });
        let (w, h) = (self.prog.width as f64, self.prog.height as f64);
        for (id, kind) in &self.prog.inputs {
            let v = match kind {
                super::InputKind::Uv => {
                    let fp = Fp { prec: self.prog.mode.prec(32) };
                    Value::V(vec![Value::F(fp.round((x as f64 + 0.5) / w)), Value::F(fp.round((y as f64 + 0.5) / h))])
                }
                super::InputKind::FragCoord => Value::V(vec![
                    Value::F(x as f64 + 0.5),
                    Value::F(y as f64 + 0.5),
                    Value::F(0.5),
                    Value::F(1.0),
                ]),
                super::InputKind::Const(v) => v.clone(),
            };
            self.mem[*id as usize] = v;
        }
    }

    /// Runs until the invocation finishes, is discarded, or needs a derivative.
    pub fn run(&mut self) -> Result<Status> {
        let module = &self.prog.lifted.module;
        loop {
            let fr = self.stack.last().ok_or_else(|| anyhow!("empty call stack"))?;
            let (func, block, pc) = (fr.func, fr.block, fr.pc);
            let blk = &module.functions[func].blocks[block];
            let inst: &'a Instruction = blk
                .instructions
                .get(pc)
                .ok_or_else(|| anyhow!("{}: fell off the end of block %{}", self.prog.label, self.prog.functions_block_label(func, block)))?;
            self.stack.last_mut().unwrap().pc += 1;
            let flow = self.exec(inst).with_context(|| {
                format!(
                    "{}: in Op{}{}",
                    self.prog.label,
                    inst.class.opname,
                    inst.result_id.map(|r| format!(" (%{r})")).unwrap_or_default()
                )
            })?;
            match flow {
                Flow::Next => {}
                Flow::Branch(label) => self.enter_block(label)?,
                Flow::Return(v) => {
                    let fr = self.stack.pop().unwrap();
                    if let Some(t) = fr.ret_target {
                        self.vals[t as usize] = v.unwrap_or(Value::Undef);
                    }
                    if self.stack.is_empty() {
                        return Ok(if self.demoted { Status::Discarded } else { Status::Finished });
                    }
                }
                Flow::Kill => return Ok(Status::Discarded),
                Flow::Yield(req) => return Ok(Status::Yield(req)),
                Flow::Call { func, args, ret } => {
                    let info = &self.prog.lifted.functions[func];
                    if info.params.len() != args.len() {
                        bail!("{}: call to %{} with {} arguments, expected {}", self.prog.label, info.id, args.len(), info.params.len());
                    }
                    if self.stack.iter().any(|f| f.func == func) {
                        bail!("{}: recursive call to function %{} is not supported", self.prog.label, info.id);
                    }
                    for (p, a) in info.params.iter().zip(args) {
                        self.vals[*p as usize] = a;
                    }
                    self.stack.push(Frame { func, block: 0, pc: 0, prev_label: 0, ret_target: ret });
                }
            }
        }
    }

    /// Delivers a derivative result and continues.
    pub fn resume(&mut self, result: u32, v: Value) -> Result<Status> {
        self.vals[result as usize] = v;
        self.run()
    }

    fn enter_block(&mut self, label: u32) -> Result<()> {
        let &(fi, bi) = self
            .prog
            .lifted
            .label_index
            .get(&label)
            .ok_or_else(|| anyhow!("{}: branch to unknown label %{label}", self.prog.label))?;
        let fr = self.stack.last_mut().unwrap();
        if fi != fr.func {
            bail!("{}: branch to label %{label} of another function", self.prog.label);
        }
        fr.prev_label = self.prog.lifted.functions[fi].blocks[fr.block];
        fr.block = bi;
        fr.pc = 0;
        let prev = fr.prev_label;
        // OpPhi: all phis of a block are evaluated simultaneously against the predecessor.
        let blk = &self.prog.lifted.module.functions[fi].blocks[bi];
        let mut assigned: Vec<(u32, Value)> = Vec::new();
        let mut pc = 0;
        while let Some(inst) = blk.instructions.get(pc) {
            match inst.class.opcode {
                Op::Line | Op::NoLine => {}
                Op::Phi => {
                    let mut chosen = None;
                    for pair in inst.operands.chunks(2) {
                        if let [Operand::IdRef(v), Operand::IdRef(parent)] = pair {
                            if *parent == prev {
                                chosen = Some(*v);
                                break;
                            }
                        }
                    }
                    let v = chosen.ok_or_else(|| {
                        anyhow!("{}: OpPhi %{} has no operand for predecessor %{prev}", self.prog.label, inst.result_id.unwrap_or(0))
                    })?;
                    assigned.push((inst.result_id.unwrap(), self.vals[v as usize].clone()));
                }
                _ => break,
            }
            pc += 1;
        }
        for (id, v) in assigned {
            self.vals[id as usize] = v;
        }
        self.stack.last_mut().unwrap().pc = pc;
        Ok(())
    }

    // ---- operand helpers ----------------------------------------------------------------

    #[inline]
    fn op_id(&self, inst: &Instruction, i: usize) -> Result<u32> {
        match inst.operands.get(i) {
            Some(Operand::IdRef(id)) => Ok(*id),
            Some(other) => bail!("operand {i} is {other:?}, expected an id"),
            None => bail!("missing operand {i}"),
        }
    }

    #[inline]
    fn op_val(&self, inst: &Instruction, i: usize) -> Result<&Value> {
        let id = self.op_id(inst, i)?;
        let v = &self.vals[id as usize];
        if matches!(v, Value::Undef) {
            // Undef is legal (OpUndef) but usually a sign of an unsupported definition.
            if !self.prog.lifted.constants.contains_key(&id) {
                bail!("operand %{id} has no value (not yet defined on this path?)");
            }
        }
        Ok(v)
    }

    #[inline]
    fn op_lit(&self, inst: &Instruction, i: usize) -> Result<u32> {
        match inst.operands.get(i) {
            Some(Operand::LiteralBit32(v)) => Ok(*v),
            Some(other) => bail!("operand {i} is {other:?}, expected a literal"),
            None => bail!("missing operand {i}"),
        }
    }

    #[inline]
    fn rt(&self, inst: &Instruction) -> Result<u32> {
        inst.result_type.ok_or_else(|| anyhow!("instruction has no result type"))
    }

    fn set(&mut self, inst: &Instruction, v: Value) -> Result<Flow> {
        let id = inst.result_id.ok_or_else(|| anyhow!("instruction has no result id"))?;
        self.vals[id as usize] = v;
        Ok(Flow::Next)
    }

    pub fn read_ptr(&self, p: &Ptr) -> Result<Value> {
        let mut v = &self.mem[p.root as usize];
        if matches!(v, Value::Undef) {
            bail!("load through pointer to variable %{} which has no storage", p.root);
        }
        for &i in &p.path {
            v = v.as_vec()?.get(i as usize).ok_or_else(|| anyhow!("index {i} out of bounds in variable %{}", p.root))?;
        }
        Ok(v.clone())
    }

    pub fn write_ptr(&mut self, p: &Ptr, val: Value) -> Result<()> {
        let mut v = &mut self.mem[p.root as usize];
        if matches!(v, Value::Undef) {
            bail!("store through pointer to variable %{} which has no storage", p.root);
        }
        for &i in &p.path {
            v = match v {
                Value::V(xs) => xs.get_mut(i as usize).ok_or_else(|| anyhow!("index {i} out of bounds in variable %{}", p.root))?,
                other => bail!("cannot index into {}", other.describe()),
            };
        }
        *v = val;
        Ok(())
    }

    fn ptr_of(&self, v: &Value) -> Result<Ptr> {
        match v {
            Value::Ptr(p) => Ok(p.clone()),
            other => bail!("expected a pointer, got {}", other.describe()),
        }
    }

    // ---- typed helpers ------------------------------------------------------------------

    fn fbin(&mut self, inst: &'a Instruction, f: fn(Fp, f64, f64) -> f64) -> Result<Flow> {
        let fp = self.prog.fp(self.rt(inst)?)?;
        let a = self.op_val(inst, 0)?;
        let b = self.op_val(inst, 1)?;
        let r = map2(a, b, &mut |x, y| Ok(Value::F(f(fp, x.as_f()?, y.as_f()?))))?;
        self.set(inst, r)
    }

    fn fun(&mut self, inst: &'a Instruction, f: fn(Fp, f64) -> f64) -> Result<Flow> {
        let fp = self.prog.fp(self.rt(inst)?)?;
        let a = self.op_val(inst, 0)?;
        let r = map1(a, &mut |x| Ok(Value::F(f(fp, x.as_f()?))))?;
        self.set(inst, r)
    }

    fn fcmp(&mut self, inst: &'a Instruction, f: fn(f64, f64) -> bool) -> Result<Flow> {
        let a = self.op_val(inst, 0)?;
        let b = self.op_val(inst, 1)?;
        let r = map2(a, b, &mut |x, y| Ok(Value::Bool(f(x.as_f()?, y.as_f()?))))?;
        self.set(inst, r)
    }

    fn signed_result(&self, inst: &Instruction) -> Result<bool> {
        match self.prog.scalar_kind(self.rt(inst)?)? {
            Kind::Int { signed } => Ok(signed),
            other => bail!("expected an integer result type, got {other:?}"),
        }
    }

    fn ibin(&mut self, inst: &'a Instruction, f: fn(u32, u32) -> u32) -> Result<Flow> {
        let signed = self.signed_result(inst)?;
        let a = self.op_val(inst, 0)?;
        let b = self.op_val(inst, 1)?;
        let r = map2(a, b, &mut |x, y| Ok(mk_int(signed, f(x.as_bits()?, y.as_bits()?))))?;
        self.set(inst, r)
    }

    fn iun(&mut self, inst: &'a Instruction, f: fn(u32) -> u32) -> Result<Flow> {
        let signed = self.signed_result(inst)?;
        let a = self.op_val(inst, 0)?;
        let r = map1(a, &mut |x| Ok(mk_int(signed, f(x.as_bits()?))))?;
        self.set(inst, r)
    }

    fn icmp(&mut self, inst: &'a Instruction, f: fn(u32, u32) -> bool) -> Result<Flow> {
        let a = self.op_val(inst, 0)?;
        let b = self.op_val(inst, 1)?;
        let r = map2(a, b, &mut |x, y| Ok(Value::Bool(f(x.as_bits()?, y.as_bits()?))))?;
        self.set(inst, r)
    }

    fn lbin(&mut self, inst: &'a Instruction, f: fn(bool, bool) -> bool) -> Result<Flow> {
        let a = self.op_val(inst, 0)?;
        let b = self.op_val(inst, 1)?;
        let r = map2(a, b, &mut |x, y| Ok(Value::Bool(f(x.as_bool()?, y.as_bool()?))))?;
        self.set(inst, r)
    }

    /// Converts scalar leaves to the result kind from bits/values.
    fn convert(&mut self, inst: &'a Instruction, f: fn(&Value, Kind, Fp) -> Result<Value>) -> Result<Flow> {
        let rt = self.rt(inst)?;
        let kind = self.prog.scalar_kind(rt)?;
        let fp = match kind {
            Kind::Float { width } => Fp { prec: self.prog.mode.prec(width) },
            _ => Fp { prec: super::value::Prec::P64 },
        };
        let a = self.op_val(inst, 0)?;
        let r = map1(a, &mut |x| f(x, kind, fp))?;
        self.set(inst, r)
    }

    fn deriv(&mut self, inst: &'a Instruction, kind: DerivKind, fine: bool) -> Result<Flow> {
        let ty = self.rt(inst)?;
        let value = self.op_val(inst, 0)?.clone();
        Ok(Flow::Yield(DerivReq { result: inst.result_id.unwrap(), ty, kind, fine, value }))
    }

    /// Parses trailing image operands and returns the integer texel offset.
    fn image_offset(&self, inst: &Instruction, start: usize) -> Result<[i64; 2]> {
        let mut offset = [0i64; 2];
        if let Some(Operand::ImageOperands(mask)) = inst.operands.get(start) {
            let bits = mask.bits();
            let mut idx = start + 1;
            // In operand order: Bias, Lod, Grad (2 ids), ConstOffset, Offset, ConstOffsets, Sample, MinLod.
            for (bit, n_ids) in [(1u32, 1usize), (2, 1), (4, 2), (8, 1), (16, 1), (32, 1), (64, 1), (128, 1)] {
                if bits & bit != 0 {
                    if bit == 8 || bit == 16 {
                        let v = self.op_val(inst, idx)?;
                        let c = v.comps();
                        if c.len() < 2 {
                            bail!("image offset must be a 2-component vector");
                        }
                        offset = [c[0].as_bits()? as i32 as i64, c[1].as_bits()? as i32 as i64];
                    }
                    if bit == 32 {
                        bail!("ConstOffsets (gather) is not supported");
                    }
                    idx += n_ids;
                }
            }
        }
        Ok(offset)
    }

    fn image_index(&self, v: &Value) -> Result<usize> {
        match v {
            Value::SampledImage(i) | Value::Image(i) => Ok(*i),
            other => bail!("expected an image, got {}", other.describe()),
        }
    }

    fn sample(&mut self, inst: &'a Instruction) -> Result<Flow> {
        let rt = self.rt(inst)?;
        let fp = self.prog.fp(rt)?;
        let idx = self.image_index(self.op_val(inst, 0)?)?;
        let coord = self.op_val(inst, 1)?.floats()?;
        if coord.len() < 2 {
            bail!("sampling coordinate must have at least 2 components");
        }
        let offset = self.image_offset(inst, 2)?;
        let img = &self.prog.images[idx];
        let t = sample_2d(&img.image, img.filter, coord[0], coord[1], offset, self.prog.weight_bits);
        let r = Value::V(t.iter().map(|c| Value::F(fp.round(*c))).collect());
        self.set(inst, r)
    }

    // ---- the instruction switch ---------------------------------------------------------

    fn exec(&mut self, inst: &'a Instruction) -> Result<Flow> {
        use std::ops::{BitAnd, BitOr, BitXor};
        match inst.class.opcode {
            Op::Nop | Op::Line | Op::NoLine | Op::SelectionMerge | Op::LoopMerge => Ok(Flow::Next),

            // ---- memory ----
            Op::Variable => {
                let id = inst.result_id.unwrap();
                let pointee = match self.prog.lifted.ty(self.rt(inst)?)? {
                    Type::Pointer { pointee, .. } => *pointee,
                    _ => bail!("OpVariable result type is not a pointer"),
                };
                let init = match inst.operands.get(1) {
                    Some(Operand::IdRef(c)) => self.vals[*c as usize].clone(),
                    _ => self.prog.zero(pointee)?,
                };
                self.mem[id as usize] = init;
                self.vals[id as usize] = Value::Ptr(Ptr { root: id, path: Vec::new() });
                Ok(Flow::Next)
            }
            Op::Load => {
                let p = self.ptr_of(self.op_val(inst, 0)?)?;
                let v = self.read_ptr(&p)?;
                self.set(inst, v)
            }
            Op::Store => {
                let p = self.ptr_of(self.op_val(inst, 0)?)?;
                let v = self.op_val(inst, 1)?.clone();
                self.write_ptr(&p, v)?;
                Ok(Flow::Next)
            }
            Op::CopyMemory => {
                let dst = self.ptr_of(self.op_val(inst, 0)?)?;
                let src = self.ptr_of(self.op_val(inst, 1)?)?;
                let v = self.read_ptr(&src)?;
                self.write_ptr(&dst, v)?;
                Ok(Flow::Next)
            }
            Op::AccessChain | Op::InBoundsAccessChain => {
                let mut p = self.ptr_of(self.op_val(inst, 0)?)?;
                for i in 1..inst.operands.len() {
                    p.path.push(self.op_val(inst, i)?.as_index()? as u32);
                }
                self.set(inst, Value::Ptr(p))
            }
            Op::ArrayLength => bail!("OpArrayLength (runtime arrays) is not supported"),

            // ---- composites ----
            Op::CompositeConstruct => {
                let rt = self.rt(inst)?;
                let is_vector = matches!(self.prog.lifted.ty(rt)?, Type::Vector { .. });
                let mut parts = Vec::with_capacity(inst.operands.len());
                for i in 0..inst.operands.len() {
                    let v = self.op_val(inst, i)?;
                    if is_vector {
                        parts.extend(v.comps().iter().cloned());
                    } else {
                        parts.push(v.clone());
                    }
                }
                self.set(inst, Value::V(parts))
            }
            Op::CompositeExtract => {
                let mut v = self.op_val(inst, 0)?;
                for i in 1..inst.operands.len() {
                    let idx = self.op_lit(inst, i)? as usize;
                    v = v.as_vec()?.get(idx).ok_or_else(|| anyhow!("CompositeExtract index {idx} out of bounds"))?;
                }
                let v = v.clone();
                self.set(inst, v)
            }
            Op::CompositeInsert => {
                let obj = self.op_val(inst, 0)?.clone();
                let mut comp = self.op_val(inst, 1)?.clone();
                {
                    let mut slot = &mut comp;
                    for i in 2..inst.operands.len() {
                        let idx = self.op_lit(inst, i)? as usize;
                        slot = match slot {
                            Value::V(xs) => xs.get_mut(idx).ok_or_else(|| anyhow!("CompositeInsert index {idx} out of bounds"))?,
                            other => bail!("cannot index into {}", other.describe()),
                        };
                    }
                    *slot = obj;
                }
                self.set(inst, comp)
            }
            Op::VectorShuffle => {
                let a = self.op_val(inst, 0)?.comps().to_vec();
                let b = self.op_val(inst, 1)?.comps().to_vec();
                let mut out = Vec::with_capacity(inst.operands.len() - 2);
                for i in 2..inst.operands.len() {
                    let idx = self.op_lit(inst, i)?;
                    out.push(if idx == 0xFFFF_FFFF {
                        Value::F(0.0)
                    } else {
                        let idx = idx as usize;
                        if idx < a.len() {
                            a[idx].clone()
                        } else {
                            b.get(idx - a.len()).ok_or_else(|| anyhow!("shuffle index {idx} out of bounds"))?.clone()
                        }
                    });
                }
                self.set(inst, Value::V(out))
            }
            Op::VectorExtractDynamic => {
                let v = self.op_val(inst, 0)?;
                let i = self.op_val(inst, 1)?.as_index()?;
                let r = v.as_vec()?.get(i).ok_or_else(|| anyhow!("dynamic index {i} out of bounds"))?.clone();
                self.set(inst, r)
            }
            Op::VectorInsertDynamic => {
                let mut v = self.op_val(inst, 0)?.clone();
                let c = self.op_val(inst, 1)?.clone();
                let i = self.op_val(inst, 2)?.as_index()?;
                match &mut v {
                    Value::V(xs) => *xs.get_mut(i).ok_or_else(|| anyhow!("dynamic index {i} out of bounds"))? = c,
                    other => bail!("cannot index into {}", other.describe()),
                }
                self.set(inst, v)
            }
            Op::CopyObject | Op::CopyLogical => {
                let v = self.op_val(inst, 0)?.clone();
                self.set(inst, v)
            }
            Op::Undef => {
                let z = self.prog.zero(self.rt(inst)?)?;
                self.set(inst, z)
            }
            Op::Transpose => {
                let m = self.op_val(inst, 0)?.as_vec()?.to_vec();
                let rows = m.first().map(|c| c.comps().len()).unwrap_or(0);
                let mut out = Vec::with_capacity(rows);
                for r in 0..rows {
                    out.push(Value::V(m.iter().map(|c| c.comps()[r].clone()).collect()));
                }
                self.set(inst, Value::V(out))
            }

            // ---- float arithmetic ----
            Op::FAdd => self.fbin(inst, |fp, a, b| fp.add(a, b)),
            Op::FSub => self.fbin(inst, |fp, a, b| fp.sub(a, b)),
            Op::FMul => self.fbin(inst, |fp, a, b| fp.mul(a, b)),
            Op::FDiv => self.fbin(inst, |fp, a, b| fp.div(a, b)),
            Op::FRem => self.fbin(inst, |fp, a, b| fp.rem(a, b)),
            Op::FMod => self.fbin(inst, |fp, a, b| fp.fmod(a, b)),
            Op::FNegate => self.fun(inst, |fp, a| fp.neg(a)),
            Op::Dot => {
                let fp = self.prog.fp(self.rt(inst)?)?;
                let a = self.op_val(inst, 0)?.floats()?;
                let b = self.op_val(inst, 1)?.floats()?;
                if a.len() != b.len() {
                    bail!("dot of vectors with {} and {} components", a.len(), b.len());
                }
                self.set(inst, Value::F(fp.dot(&a, &b)))
            }
            Op::VectorTimesScalar | Op::MatrixTimesScalar => {
                let fp = self.prog.fp(self.rt(inst)?)?;
                let v = self.op_val(inst, 0)?;
                let s = self.op_val(inst, 1)?.as_f()?;
                let r = map1(v, &mut |x| Ok(Value::F(fp.mul(x.as_f()?, s))))?;
                self.set(inst, r)
            }
            Op::VectorTimesMatrix => {
                let fp = self.prog.fp(self.rt(inst)?)?;
                let v = self.op_val(inst, 0)?.floats()?;
                let m = self.op_val(inst, 1)?.as_vec()?;
                let mut out = Vec::with_capacity(m.len());
                for col in m {
                    let c = col.floats()?;
                    if c.len() != v.len() {
                        bail!("vector*matrix size mismatch");
                    }
                    out.push(Value::F(fp.dot(&v, &c)));
                }
                self.set(inst, Value::V(out))
            }
            Op::MatrixTimesVector => {
                let fp = self.prog.fp(self.rt(inst)?)?;
                let m = self.op_val(inst, 0)?.as_vec()?;
                let v = self.op_val(inst, 1)?.floats()?;
                let r = mat_vec(fp, m, &v)?;
                self.set(inst, r)
            }
            Op::MatrixTimesMatrix => {
                let fp = self.prog.fp(self.rt(inst)?)?;
                let a = self.op_val(inst, 0)?.as_vec()?;
                let b = self.op_val(inst, 1)?.as_vec()?;
                let mut out = Vec::with_capacity(b.len());
                for col in b {
                    out.push(mat_vec(fp, a, &col.floats()?)?);
                }
                self.set(inst, Value::V(out))
            }
            Op::OuterProduct => {
                let fp = self.prog.fp(self.rt(inst)?)?;
                let a = self.op_val(inst, 0)?.floats()?;
                let b = self.op_val(inst, 1)?.floats()?;
                let cols = b.iter().map(|bj| Value::V(a.iter().map(|ai| Value::F(fp.mul(*ai, *bj))).collect())).collect();
                self.set(inst, Value::V(cols))
            }

            // ---- integer arithmetic ----
            Op::IAdd => self.ibin(inst, u32::wrapping_add),
            Op::ISub => self.ibin(inst, u32::wrapping_sub),
            Op::IMul => self.ibin(inst, u32::wrapping_mul),
            Op::SDiv => self.ibin(inst, |a, b| {
                let (a, b) = (a as i32, b as i32);
                if b == 0 { 0 } else { a.wrapping_div(b) as u32 }
            }),
            Op::UDiv => self.ibin(inst, |a, b| if b == 0 { 0 } else { a / b }),
            Op::SRem => self.ibin(inst, |a, b| {
                let (a, b) = (a as i32, b as i32);
                if b == 0 { 0 } else { a.wrapping_rem(b) as u32 }
            }),
            Op::SMod => self.ibin(inst, |a, b| {
                // Result takes the sign of the divisor (GLSL `%` on ints is SRem; `mod` is this).
                let (a, b) = (a as i32, b as i32);
                if b == 0 {
                    0
                } else {
                    let r = a.wrapping_rem(b);
                    (if r != 0 && ((r < 0) != (b < 0)) { r.wrapping_add(b) } else { r }) as u32
                }
            }),
            Op::UMod => self.ibin(inst, |a, b| if b == 0 { 0 } else { a % b }),
            Op::SNegate => self.iun(inst, |a| (a as i32).wrapping_neg() as u32),
            Op::BitwiseAnd => self.ibin(inst, u32::bitand),
            Op::BitwiseOr => self.ibin(inst, u32::bitor),
            Op::BitwiseXor => self.ibin(inst, u32::bitxor),
            Op::Not => self.iun(inst, |a| !a),
            Op::ShiftLeftLogical => self.ibin(inst, |a, b| if b >= 32 { 0 } else { a << b }),
            Op::ShiftRightLogical => self.ibin(inst, |a, b| if b >= 32 { 0 } else { a >> b }),
            Op::ShiftRightArithmetic => self.ibin(inst, |a, b| ((a as i32) >> b.min(31)) as u32),
            Op::BitCount => self.iun(inst, |a| a.count_ones()),
            Op::BitReverse => self.iun(inst, |a| a.reverse_bits()),
            Op::BitFieldUExtract | Op::BitFieldSExtract => {
                let signed_result = self.signed_result(inst)?;
                let arith = inst.class.opcode == Op::BitFieldSExtract;
                let base = self.op_val(inst, 0)?.clone();
                let off = self.op_val(inst, 1)?.as_bits()?;
                let cnt = self.op_val(inst, 2)?.as_bits()?;
                let r = map1(&base, &mut |x| {
                    let b = x.as_bits()?;
                    let v = if cnt == 0 {
                        0
                    } else if arith {
                        (((b << (32 - off - cnt).min(31)) as i32) >> (32 - cnt).min(31)) as u32
                    } else {
                        (b >> off) & (u32::MAX >> (32 - cnt))
                    };
                    Ok(mk_int(signed_result, v))
                })?;
                self.set(inst, r)
            }
            Op::BitFieldInsert => {
                let signed_result = self.signed_result(inst)?;
                let base = self.op_val(inst, 0)?.clone();
                let ins = self.op_val(inst, 1)?.clone();
                let off = self.op_val(inst, 2)?.as_bits()?;
                let cnt = self.op_val(inst, 3)?.as_bits()?;
                let mask = if cnt >= 32 { u32::MAX } else { ((1u32 << cnt) - 1) << off.min(31) };
                let r = map2(&base, &ins, &mut |b, i| Ok(mk_int(signed_result, (b.as_bits()? & !mask) | ((i.as_bits()? << off.min(31)) & mask))))?;
                self.set(inst, r)
            }

            // ---- comparisons ----
            Op::FOrdEqual => self.fcmp(inst, |a, b| a == b),
            Op::FOrdNotEqual => self.fcmp(inst, |a, b| a != b && !a.is_nan() && !b.is_nan()),
            Op::FOrdLessThan => self.fcmp(inst, |a, b| a < b),
            Op::FOrdGreaterThan => self.fcmp(inst, |a, b| a > b),
            Op::FOrdLessThanEqual => self.fcmp(inst, |a, b| a <= b),
            Op::FOrdGreaterThanEqual => self.fcmp(inst, |a, b| a >= b),
            Op::FUnordEqual => self.fcmp(inst, |a, b| a == b || a.is_nan() || b.is_nan()),
            Op::FUnordNotEqual => self.fcmp(inst, |a, b| a != b),
            Op::FUnordLessThan => self.fcmp(inst, |a, b| !(a >= b)),
            Op::FUnordGreaterThan => self.fcmp(inst, |a, b| !(a <= b)),
            Op::FUnordLessThanEqual => self.fcmp(inst, |a, b| !(a > b)),
            Op::FUnordGreaterThanEqual => self.fcmp(inst, |a, b| !(a < b)),
            Op::IEqual => self.icmp(inst, |a, b| a == b),
            Op::INotEqual => self.icmp(inst, |a, b| a != b),
            Op::ULessThan => self.icmp(inst, |a, b| a < b),
            Op::UGreaterThan => self.icmp(inst, |a, b| a > b),
            Op::ULessThanEqual => self.icmp(inst, |a, b| a <= b),
            Op::UGreaterThanEqual => self.icmp(inst, |a, b| a >= b),
            Op::SLessThan => self.icmp(inst, |a, b| (a as i32) < (b as i32)),
            Op::SGreaterThan => self.icmp(inst, |a, b| (a as i32) > (b as i32)),
            Op::SLessThanEqual => self.icmp(inst, |a, b| (a as i32) <= (b as i32)),
            Op::SGreaterThanEqual => self.icmp(inst, |a, b| (a as i32) >= (b as i32)),
            Op::LogicalEqual => self.lbin(inst, |a, b| a == b),
            Op::LogicalNotEqual => self.lbin(inst, |a, b| a != b),
            Op::LogicalAnd => self.lbin(inst, |a, b| a && b),
            Op::LogicalOr => self.lbin(inst, |a, b| a || b),
            Op::LogicalNot => {
                let a = self.op_val(inst, 0)?;
                let r = map1(a, &mut |x| Ok(Value::Bool(!x.as_bool()?)))?;
                self.set(inst, r)
            }
            Op::Select => {
                let c = self.op_val(inst, 0)?;
                let a = self.op_val(inst, 1)?;
                let b = self.op_val(inst, 2)?;
                let r = match c {
                    Value::V(_) => map3(c, a, b, &mut |c, a, b| Ok(if c.as_bool()? { a.clone() } else { b.clone() }))?,
                    c => {
                        if c.as_bool()? {
                            a.clone()
                        } else {
                            b.clone()
                        }
                    }
                };
                self.set(inst, r)
            }
            Op::Any | Op::All => {
                let all = inst.class.opcode == Op::All;
                let v = self.op_val(inst, 0)?.comps();
                let mut r = all;
                for c in v {
                    let b = c.as_bool()?;
                    r = if all { r && b } else { r || b };
                }
                self.set(inst, Value::Bool(r))
            }
            Op::IsNan => {
                let a = self.op_val(inst, 0)?;
                let r = map1(a, &mut |x| Ok(Value::Bool(x.as_f()?.is_nan())))?;
                self.set(inst, r)
            }
            Op::IsInf => {
                let a = self.op_val(inst, 0)?;
                let r = map1(a, &mut |x| Ok(Value::Bool(x.as_f()?.is_infinite())))?;
                self.set(inst, r)
            }

            // ---- conversions ----
            Op::ConvertFToS => self.convert(inst, |x, _, _| Ok(Value::I32(x.as_f()? as i32))),
            Op::ConvertFToU => self.convert(inst, |x, _, _| Ok(Value::U32(x.as_f()? as u32))),
            Op::ConvertSToF => self.convert(inst, |x, _, fp| Ok(Value::F(fp.round(x.as_bits()? as i32 as f64)))),
            Op::ConvertUToF => self.convert(inst, |x, _, fp| Ok(Value::F(fp.round(x.as_bits()? as f64)))),
            Op::FConvert => self.convert(inst, |x, _, fp| Ok(Value::F(fp.round(x.as_f()?)))),
            Op::QuantizeToF16 => self.convert(inst, |x, _, fp| Ok(Value::F(fp.round(super::value::r16(x.as_f()? as f32))))),
            Op::SConvert | Op::UConvert => self.convert(inst, |x, k, _| match k {
                Kind::Int { signed } => Ok(mk_int(signed, x.as_bits()?)),
                other => bail!("integer conversion to {other:?}"),
            }),
            Op::Bitcast => {
                let rt = self.rt(inst)?;
                let kind = self.prog.scalar_kind(rt)?;
                let a = self.op_val(inst, 0)?;
                let n_in = a.comps().len();
                let n_out = match self.prog.lifted.ty(rt)? {
                    Type::Vector { count, .. } => *count as usize,
                    _ => 1,
                };
                if n_in != n_out {
                    bail!("OpBitcast between {n_in} and {n_out} components is not supported");
                }
                let r = map1(a, &mut |x| {
                    let bits = match x {
                        Value::F(f) => (*f as f32).to_bits(),
                        other => other.as_bits()?,
                    };
                    Ok(match kind {
                        Kind::Float { width: 32 } => Value::F(f32::from_bits(bits) as f64),
                        Kind::Float { width } => bail!("OpBitcast to {width}-bit float"),
                        Kind::Int { signed } => mk_int(signed, bits),
                        Kind::Bool => bail!("OpBitcast to bool"),
                    })
                })?;
                self.set(inst, r)
            }

            // ---- extended instructions ----
            Op::ExtInst => {
                let set = self.op_id(inst, 0)?;
                let n = match inst.operands.get(1) {
                    Some(Operand::LiteralExtInstInteger(n)) => *n,
                    _ => bail!("OpExtInst without instruction number"),
                };
                if Some(set) != self.prog.glsl_set {
                    let name = self.prog.lifted.ext_inst_imports.get(&set).map(|s| s.as_str()).unwrap_or("?");
                    bail!("extended instruction set {name:?} is not supported (only GLSL.std.450)");
                }
                let r = ext::glsl(self, inst, n)?;
                self.set(inst, r)
            }

            // ---- images ----
            Op::SampledImage => {
                let idx = self.image_index(self.op_val(inst, 0)?)?;
                self.set(inst, Value::SampledImage(idx))
            }
            Op::Image => {
                let idx = self.image_index(self.op_val(inst, 0)?)?;
                self.set(inst, Value::Image(idx))
            }
            Op::ImageSampleImplicitLod | Op::ImageSampleExplicitLod => self.sample(inst),
            Op::ImageFetch => {
                let rt = self.rt(inst)?;
                let fp = self.prog.fp(rt)?;
                let idx = self.image_index(self.op_val(inst, 0)?)?;
                let c = self.op_val(inst, 1)?.comps().to_vec();
                if c.len() < 2 {
                    bail!("texelFetch coordinate must be an ivec2");
                }
                let offset = self.image_offset(inst, 2)?;
                let (x, y) = (c[0].as_bits()? as i32 as i64 + offset[0], c[1].as_bits()? as i32 as i64 + offset[1]);
                let t = fetch_2d(&self.prog.images[idx].image, x, y);
                let r = Value::V(t.iter().map(|v| Value::F(fp.round(*v))).collect());
                self.set(inst, r)
            }
            Op::ImageQuerySize | Op::ImageQuerySizeLod => {
                let rt = self.rt(inst)?;
                let signed = matches!(self.prog.scalar_kind(rt)?, Kind::Int { signed: true });
                let idx = self.image_index(self.op_val(inst, 0)?)?;
                let img = &self.prog.images[idx].image;
                let r = Value::V(vec![mk_int(signed, img.width as u32), mk_int(signed, img.height as u32)]);
                self.set(inst, r)
            }
            Op::ImageQueryLevels => {
                let signed = matches!(self.prog.scalar_kind(self.rt(inst)?)?, Kind::Int { signed: true });
                self.set(inst, mk_int(signed, 1))
            }

            // ---- derivatives ----
            Op::DPdx | Op::DPdxCoarse => self.deriv(inst, DerivKind::Dx, false),
            Op::DPdy | Op::DPdyCoarse => self.deriv(inst, DerivKind::Dy, false),
            Op::Fwidth | Op::FwidthCoarse => self.deriv(inst, DerivKind::Fwidth, false),
            Op::DPdxFine => self.deriv(inst, DerivKind::Dx, true),
            Op::DPdyFine => self.deriv(inst, DerivKind::Dy, true),
            Op::FwidthFine => self.deriv(inst, DerivKind::Fwidth, true),

            // ---- control flow ----
            Op::Branch => Ok(Flow::Branch(self.op_id(inst, 0)?)),
            Op::BranchConditional => {
                let c = self.op_val(inst, 0)?.as_bool()?;
                Ok(Flow::Branch(self.op_id(inst, if c { 1 } else { 2 })?))
            }
            Op::Switch => {
                let sel = self.op_val(inst, 0)?.as_bits()?;
                let mut target = self.op_id(inst, 1)?;
                let mut i = 2;
                while i + 1 < inst.operands.len() {
                    let lit = match &inst.operands[i] {
                        Operand::LiteralBit32(v) => *v,
                        Operand::LiteralBit64(v) => *v as u32,
                        other => bail!("OpSwitch literal is {other:?}"),
                    };
                    if lit == sel {
                        target = self.op_id(inst, i + 1)?;
                        break;
                    }
                    i += 2;
                }
                Ok(Flow::Branch(target))
            }
            Op::Phi => bail!("OpPhi outside the head of a block"),
            Op::Return => Ok(Flow::Return(None)),
            Op::ReturnValue => Ok(Flow::Return(Some(self.op_val(inst, 0)?.clone()))),
            Op::Kill | Op::TerminateInvocation => Ok(Flow::Kill),
            Op::DemoteToHelperInvocation => {
                self.demoted = true;
                Ok(Flow::Next)
            }
            Op::IsHelperInvocationEXT => {
                let d = self.demoted;
                self.set(inst, Value::Bool(d))
            }
            Op::Unreachable => bail!("OpUnreachable was executed"),
            Op::FunctionCall => {
                let fid = self.op_id(inst, 0)?;
                let func = *self
                    .prog
                    .lifted
                    .function_index
                    .get(&fid)
                    .ok_or_else(|| anyhow!("call to unknown function %{fid}"))?;
                let mut args = Vec::with_capacity(inst.operands.len() - 1);
                for i in 1..inst.operands.len() {
                    args.push(self.op_val(inst, i)?.clone());
                }
                Ok(Flow::Call { func, args, ret: inst.result_id })
            }

            other => bail!("unsupported opcode Op{} ({other:?})", inst.class.opname),
        }
    }
}

/// `m * v` for a column-major matrix: row `i` of the result is `dot(row_i(m), v)`.
fn mat_vec(fp: Fp, m: &[Value], v: &[f64]) -> Result<Value> {
    if m.len() != v.len() {
        bail!("matrix*vector size mismatch: {} columns vs {} components", m.len(), v.len());
    }
    let cols: Vec<Vec<f64>> = m.iter().map(|c| c.floats()).collect::<Result<_>>()?;
    let rows = cols.first().map(|c| c.len()).unwrap_or(0);
    let mut out = Vec::with_capacity(rows);
    for r in 0..rows {
        let row: Vec<f64> = cols.iter().map(|c| c[r]).collect();
        out.push(Value::F(fp.dot(&row, v)));
    }
    Ok(Value::V(out))
}

impl<'a> Program<'a> {
    fn functions_block_label(&self, func: usize, block: usize) -> u32 {
        self.lifted.functions.get(func).and_then(|f| f.blocks.get(block).copied()).unwrap_or(0)
    }
}

/// Computes the derivative for `lane` (0 = top-left, 1 = top-right, 2 = bottom-left,
/// 3 = bottom-right) from the operand values of the four lanes (`None` = lane already dead).
pub fn derivative(req: &DerivReq, lane: usize, vals: &[Option<&Value>], fp: Fp, dead: &mut usize) -> Result<Value> {
    let (row, col) = (lane >> 1, lane & 1);
    let dx_pair = if req.fine { (row * 2, row * 2 + 1) } else { (0, 1) };
    let dy_pair = if req.fine { (col, col + 2) } else { (0, 2) };
    let own = vals[lane].ok_or_else(|| anyhow!("derivative requested by a dead lane"))?;
    let mut diff = |pair: (usize, usize)| -> Result<Value> {
        match (vals[pair.0], vals[pair.1]) {
            (Some(a), Some(b)) => map2(b, a, &mut |x, y| Ok(Value::F(fp.sub(x.as_f()?, y.as_f()?)))),
            _ => {
                *dead += 1;
                map1(own, &mut |_| Ok(Value::F(0.0)))
            }
        }
    };
    Ok(match req.kind {
        DerivKind::Dx => diff(dx_pair)?,
        DerivKind::Dy => diff(dy_pair)?,
        DerivKind::Fwidth => {
            let dx = diff(dx_pair)?;
            let dy = diff(dy_pair)?;
            map2(&dx, &dy, &mut |a, b| Ok(Value::F(fp.add(fp.abs(a.as_f()?), fp.abs(b.as_f()?)))))?
        }
    })
}
