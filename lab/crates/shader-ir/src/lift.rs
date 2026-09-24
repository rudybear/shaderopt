//! Lossless lift of a SPIR-V module.
//!
//! The IR *is* `rspirv::dr::Module`; nothing is copied into a second representation. `Lifted`
//! adds side tables derived from the module (types, constants, def-use, block order, structured
//! control flow, entry points, interface variables, names). Edits mutate `module` in place;
//! call [`Lifted::reanalyze`] afterwards to rebuild the tables, and [`Lifted::assemble`] to emit
//! words. Instruction order is never changed by the lift, so `assemble()` reproduces the input
//! body word for word. Only header word 2 (the generator magic) differs: rspirv writes its own
//! generator id (`0xf0000`) in place of the producer's (glslang: `0x8000b`).

use anyhow::{anyhow, bail, Result};
use rspirv::binary::Assemble;
use rspirv::dr::{self, Operand};
use spirv::{BuiltIn, Decoration, ExecutionModel, Op, StorageClass};
use std::collections::{BTreeMap, HashMap};

/// A SPIR-V type, keyed by its result id in [`Lifted::types`].
#[derive(Clone, Debug, PartialEq)]
pub enum Type {
    Void,
    Bool,
    Int { width: u32, signed: bool },
    Float { width: u32 },
    Vector { elem: u32, count: u32 },
    /// `column` is the column vector type id; `columns` the number of columns.
    Matrix { column: u32, columns: u32 },
    Array { elem: u32, len: u32 },
    RuntimeArray { elem: u32 },
    Struct { members: Vec<u32> },
    Pointer { storage: StorageClass, pointee: u32 },
    Image {
        sampled_type: u32,
        dim: spirv::Dim,
        depth: u32,
        arrayed: u32,
        ms: u32,
        sampled: u32,
        format: spirv::ImageFormat,
    },
    Sampler,
    SampledImage { image: u32 },
    Function { ret: u32, params: Vec<u32> },
    /// A type opcode this crate does not model; kept so the lift stays lossless.
    Other(Op),
}

/// A constant, keyed by result id in [`Lifted::constants`]. Bit patterns are kept raw; the
/// interpreter turns them into typed values using the constant's type.
#[derive(Clone, Debug, PartialEq)]
pub struct Constant {
    pub ty: u32,
    pub kind: ConstKind,
    /// True for `OpSpecConstant*` (the default value is used).
    pub spec: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ConstKind {
    Bool(bool),
    Bits32(u32),
    Bits64(u64),
    Composite(Vec<u32>),
    Null,
    Undef,
    /// `OpSpecConstantOp`: not evaluated by this crate.
    SpecOp(Op),
}

/// Where an id is defined or used.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Site {
    /// Index into `module.types_global_values`.
    Global(usize),
    /// Index into `module.entry_points`.
    EntryPoint(usize),
    /// Index into `module.annotations`.
    Annotation(usize),
    /// Index into `module.debug_names`.
    DebugName(usize),
    /// `module.functions[f].def`.
    FunctionDef(usize),
    /// `module.functions[f].parameters[p]`.
    Parameter(usize, usize),
    /// `module.functions[f].blocks[b].label`.
    Label(usize, usize),
    /// `module.functions[f].blocks[b].instructions[i]`.
    Inst(usize, usize, usize),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Decor {
    pub decoration: Decoration,
    pub operands: Vec<Operand>,
}

/// Structured control-flow info for a header block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Structured {
    pub merge: u32,
    /// `Some` for loop headers (`OpLoopMerge`), `None` for selection headers.
    pub continue_target: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct FunctionInfo {
    pub id: u32,
    pub ty: u32,
    pub name: Option<String>,
    pub params: Vec<u32>,
    /// Block labels in module order.
    pub blocks: Vec<u32>,
    pub label_index: HashMap<u32, usize>,
    /// Header label -> merge info.
    pub headers: HashMap<u32, Structured>,
}

#[derive(Clone, Debug)]
pub struct EntryPoint {
    pub model: ExecutionModel,
    pub function: u32,
    pub name: String,
    pub interface: Vec<u32>,
}

/// A module-scope `OpVariable` with the decorations the lab cares about.
#[derive(Clone, Debug)]
pub struct InterfaceVar {
    pub id: u32,
    pub name: Option<String>,
    pub storage: StorageClass,
    /// The pointer type id of the variable.
    pub ty: u32,
    /// The pointee type id.
    pub pointee: u32,
    pub location: Option<u32>,
    pub binding: Option<u32>,
    pub descriptor_set: Option<u32>,
    pub builtin: Option<BuiltIn>,
    /// `Block` decoration on the pointee struct.
    pub block: bool,
    pub relaxed_precision: bool,
    pub initializer: Option<u32>,
}

pub struct Lifted {
    pub module: dr::Module,
    pub types: HashMap<u32, Type>,
    pub constants: HashMap<u32, Constant>,
    /// Result type of every id that has one.
    pub result_types: HashMap<u32, u32>,
    pub defs: HashMap<u32, Site>,
    pub uses: HashMap<u32, Vec<Site>>,
    pub functions: Vec<FunctionInfo>,
    pub function_index: HashMap<u32, usize>,
    /// Label id -> (function index, block index).
    pub label_index: HashMap<u32, (usize, usize)>,
    pub entry_points: Vec<EntryPoint>,
    pub variables: Vec<InterfaceVar>,
    pub names: HashMap<u32, String>,
    pub member_names: HashMap<(u32, u32), String>,
    pub decorations: HashMap<u32, Vec<Decor>>,
    pub member_decorations: HashMap<(u32, u32), Vec<Decor>>,
    /// `OpExtInstImport` result id -> set name.
    pub ext_inst_imports: HashMap<u32, String>,
    pub bound: u32,
}

impl Lifted {
    pub fn load(words: &[u32]) -> Result<Self> {
        let module = dr::load_words(words).map_err(|e| anyhow!("rspirv load failed: {e}"))?;
        Self::from_module(module)
    }

    pub fn from_module(module: dr::Module) -> Result<Self> {
        let mut l = Lifted {
            module,
            types: HashMap::new(),
            constants: HashMap::new(),
            result_types: HashMap::new(),
            defs: HashMap::new(),
            uses: HashMap::new(),
            functions: Vec::new(),
            function_index: HashMap::new(),
            label_index: HashMap::new(),
            entry_points: Vec::new(),
            variables: Vec::new(),
            names: HashMap::new(),
            member_names: HashMap::new(),
            decorations: HashMap::new(),
            member_decorations: HashMap::new(),
            ext_inst_imports: HashMap::new(),
            bound: 0,
        };
        l.reanalyze()?;
        Ok(l)
    }

    pub fn assemble(&self) -> Vec<u32> {
        self.module.assemble()
    }

    /// Rebuilds every side table from `self.module`.
    pub fn reanalyze(&mut self) -> Result<()> {
        // The module is moved out while the tables are built so that the borrow checker allows
        // mutating the tables; it is put back unchanged on every path.
        let module = std::mem::take(&mut self.module);
        let r = self.reanalyze_inner(&module);
        self.module = module;
        r
    }

    fn reanalyze_inner(&mut self, module: &dr::Module) -> Result<()> {
        self.types.clear();
        self.constants.clear();
        self.result_types.clear();
        self.defs.clear();
        self.uses.clear();
        self.functions.clear();
        self.function_index.clear();
        self.label_index.clear();
        self.entry_points.clear();
        self.variables.clear();
        self.names.clear();
        self.member_names.clear();
        self.decorations.clear();
        self.member_decorations.clear();
        self.ext_inst_imports.clear();
        self.bound = module.header.as_ref().map(|h| h.bound).unwrap_or(0);

        // Names.
        for inst in &module.debug_names {
            match inst.class.opcode {
                Op::Name => {
                    if let [Operand::IdRef(id), Operand::LiteralString(s)] = inst.operands.as_slice() {
                        self.names.insert(*id, s.clone());
                    }
                }
                Op::MemberName => {
                    if let [Operand::IdRef(id), Operand::LiteralBit32(m), Operand::LiteralString(s)] =
                        inst.operands.as_slice()
                    {
                        self.member_names.insert((*id, *m), s.clone());
                    }
                }
                _ => {}
            }
        }
        // Decorations.
        for inst in &module.annotations {
            match inst.class.opcode {
                Op::Decorate => {
                    if let [Operand::IdRef(id), Operand::Decoration(d), rest @ ..] = inst.operands.as_slice() {
                        self.decorations.entry(*id).or_default().push(Decor { decoration: *d, operands: rest.to_vec() });
                    }
                }
                Op::MemberDecorate => {
                    if let [Operand::IdRef(id), Operand::LiteralBit32(m), Operand::Decoration(d), rest @ ..] =
                        inst.operands.as_slice()
                    {
                        self.member_decorations
                            .entry((*id, *m))
                            .or_default()
                            .push(Decor { decoration: *d, operands: rest.to_vec() });
                    }
                }
                _ => {}
            }
        }
        for inst in &module.ext_inst_imports {
            if let (Some(id), Some(Operand::LiteralString(s))) = (inst.result_id, inst.operands.first()) {
                self.ext_inst_imports.insert(id, s.clone());
            }
        }
        for inst in &module.entry_points {
            if let [Operand::ExecutionModel(model), Operand::IdRef(f), Operand::LiteralString(name), rest @ ..] =
                inst.operands.as_slice()
            {
                self.entry_points.push(EntryPoint {
                    model: *model,
                    function: *f,
                    name: name.clone(),
                    interface: rest.iter().filter_map(|o| o.id_ref_any()).collect(),
                });
            }
        }

        // Types, constants, global variables.
        for (i, inst) in module.types_global_values.iter().enumerate() {
            let site = Site::Global(i);
            self.record_def_use(inst, site);
            let Some(id) = inst.result_id else { continue };
            if let Some(ty) = Self::type_of_inst(inst) {
                self.types.insert(id, ty);
                continue;
            }
            let ty = inst.result_type.unwrap_or(0);
            let op = inst.class.opcode;
            let kind = match op {
                Op::ConstantTrue | Op::SpecConstantTrue => Some(ConstKind::Bool(true)),
                Op::ConstantFalse | Op::SpecConstantFalse => Some(ConstKind::Bool(false)),
                Op::Constant | Op::SpecConstant => match inst.operands.first() {
                    Some(Operand::LiteralBit32(v)) => Some(ConstKind::Bits32(*v)),
                    Some(Operand::LiteralBit64(v)) => Some(ConstKind::Bits64(*v)),
                    _ => bail!("OpConstant %{id} has an unexpected literal operand"),
                },
                Op::ConstantComposite | Op::SpecConstantComposite => {
                    Some(ConstKind::Composite(inst.operands.iter().filter_map(|o| o.id_ref_any()).collect()))
                }
                Op::ConstantNull => Some(ConstKind::Null),
                Op::Undef => Some(ConstKind::Undef),
                Op::SpecConstantOp => Some(ConstKind::SpecOp(match inst.operands.first() {
                    Some(Operand::LiteralSpecConstantOpInteger(o)) => *o,
                    _ => Op::Nop,
                })),
                _ => None,
            };
            if let Some(kind) = kind {
                let spec = matches!(
                    op,
                    Op::SpecConstant | Op::SpecConstantTrue | Op::SpecConstantFalse | Op::SpecConstantComposite | Op::SpecConstantOp
                );
                self.constants.insert(id, Constant { ty, kind, spec });
            }
        }
        // Global variables need the type table, so a second pass.
        for inst in &module.types_global_values {
            if inst.class.opcode != Op::Variable {
                continue;
            }
            let id = inst.result_id.unwrap();
            let ty = inst.result_type.unwrap();
            let storage = match inst.operands.first() {
                Some(Operand::StorageClass(s)) => *s,
                _ => bail!("OpVariable %{id} has no storage class"),
            };
            let pointee = match self.types.get(&ty) {
                Some(Type::Pointer { pointee, .. }) => *pointee,
                _ => bail!("OpVariable %{id} result type %{ty} is not a pointer type"),
            };
            let initializer = inst.operands.get(1).and_then(|o| o.id_ref_any());
            let block = self.has_decoration(pointee, Decoration::Block) || self.has_decoration(pointee, Decoration::BufferBlock);
            self.variables.push(InterfaceVar {
                id,
                name: self.names.get(&id).cloned(),
                storage,
                ty,
                pointee,
                location: self.decoration_u32(id, Decoration::Location),
                binding: self.decoration_u32(id, Decoration::Binding),
                descriptor_set: self.decoration_u32(id, Decoration::DescriptorSet),
                builtin: self.builtin_of(id),
                block,
                relaxed_precision: self.has_decoration(id, Decoration::RelaxedPrecision),
                initializer,
            });
        }
        for (i, inst) in module.entry_points.iter().enumerate() {
            self.record_uses(inst, Site::EntryPoint(i));
        }
        for (i, inst) in module.annotations.iter().enumerate() {
            self.record_uses(inst, Site::Annotation(i));
        }
        for (i, inst) in module.debug_names.iter().enumerate() {
            self.record_uses(inst, Site::DebugName(i));
        }

        // Functions.
        for (fi, f) in module.functions.iter().enumerate() {
            let def = f.def.as_ref().ok_or_else(|| anyhow!("function {fi} has no OpFunction"))?;
            let id = def.result_id.ok_or_else(|| anyhow!("OpFunction without result id"))?;
            let ty = match def.operands.get(1) {
                Some(Operand::IdRef(t)) => *t,
                _ => bail!("OpFunction %{id} has no function type operand"),
            };
            self.record_def_use(def, Site::FunctionDef(fi));
            let mut params = Vec::new();
            for (pi, p) in f.parameters.iter().enumerate() {
                self.record_def_use(p, Site::Parameter(fi, pi));
                params.push(p.result_id.unwrap_or(0));
            }
            let mut blocks = Vec::new();
            let mut label_index = HashMap::new();
            let mut headers = HashMap::new();
            for (bi, b) in f.blocks.iter().enumerate() {
                let label = b
                    .label
                    .as_ref()
                    .and_then(|l| l.result_id)
                    .ok_or_else(|| anyhow!("block {bi} of function %{id} has no label"))?;
                if let Some(l) = &b.label {
                    self.record_def_use(l, Site::Label(fi, bi));
                }
                blocks.push(label);
                label_index.insert(label, bi);
                self.label_index.insert(label, (fi, bi));
                for (ii, inst) in b.instructions.iter().enumerate() {
                    self.record_def_use(inst, Site::Inst(fi, bi, ii));
                    match inst.class.opcode {
                        Op::SelectionMerge => {
                            if let Some(Operand::IdRef(m)) = inst.operands.first() {
                                headers.insert(label, Structured { merge: *m, continue_target: None });
                            }
                        }
                        Op::LoopMerge => {
                            if let [Operand::IdRef(m), Operand::IdRef(c), ..] = inst.operands.as_slice() {
                                headers.insert(label, Structured { merge: *m, continue_target: Some(*c) });
                            }
                        }
                        _ => {}
                    }
                }
            }
            self.function_index.insert(id, fi);
            self.functions.push(FunctionInfo {
                id,
                ty,
                name: self.names.get(&id).cloned(),
                params,
                blocks,
                label_index,
                headers,
            });
        }
        Ok(())
    }

    fn record_def_use(&mut self, inst: &dr::Instruction, site: Site) {
        if let Some(id) = inst.result_id {
            self.defs.insert(id, site);
        }
        if let Some(t) = inst.result_type {
            if let Some(id) = inst.result_id {
                self.result_types.insert(id, t);
            }
            self.uses.entry(t).or_default().push(site);
        }
        self.record_uses(inst, site);
    }

    fn record_uses(&mut self, inst: &dr::Instruction, site: Site) {
        for o in &inst.operands {
            if let Some(id) = o.id_ref_any() {
                self.uses.entry(id).or_default().push(site);
            }
        }
    }

    fn type_of_inst(inst: &dr::Instruction) -> Option<Type> {
        let ops = &inst.operands;
        let id = |i: usize| match ops.get(i) {
            Some(Operand::IdRef(x)) => *x,
            _ => 0,
        };
        let lit = |i: usize| match ops.get(i) {
            Some(Operand::LiteralBit32(x)) => *x,
            _ => 0,
        };
        Some(match inst.class.opcode {
            Op::TypeVoid => Type::Void,
            Op::TypeBool => Type::Bool,
            Op::TypeInt => Type::Int { width: lit(0), signed: lit(1) != 0 },
            Op::TypeFloat => Type::Float { width: lit(0) },
            Op::TypeVector => Type::Vector { elem: id(0), count: lit(1) },
            Op::TypeMatrix => Type::Matrix { column: id(0), columns: lit(1) },
            // Length is a constant id; resolved in `array_len`.
            Op::TypeArray => Type::Array { elem: id(0), len: id(1) },
            Op::TypeRuntimeArray => Type::RuntimeArray { elem: id(0) },
            Op::TypeStruct => Type::Struct { members: ops.iter().filter_map(|o| o.id_ref_any()).collect() },
            Op::TypePointer => Type::Pointer {
                storage: match ops.first() {
                    Some(Operand::StorageClass(s)) => *s,
                    _ => StorageClass::Function,
                },
                pointee: id(1),
            },
            Op::TypeImage => Type::Image {
                sampled_type: id(0),
                dim: match ops.get(1) {
                    Some(Operand::Dim(d)) => *d,
                    _ => spirv::Dim::Dim2D,
                },
                depth: lit(2),
                arrayed: lit(3),
                ms: lit(4),
                sampled: lit(5),
                format: match ops.get(6) {
                    Some(Operand::ImageFormat(f)) => *f,
                    _ => spirv::ImageFormat::Unknown,
                },
            },
            Op::TypeSampler => Type::Sampler,
            Op::TypeSampledImage => Type::SampledImage { image: id(0) },
            Op::TypeFunction => Type::Function { ret: id(0), params: ops.iter().skip(1).filter_map(|o| o.id_ref_any()).collect() },
            op if inst.class.opname.starts_with("Type") => Type::Other(op),
            _ => return None,
        })
    }

    // ---- queries -------------------------------------------------------------------------

    pub fn ty(&self, id: u32) -> Result<&Type> {
        self.types.get(&id).ok_or_else(|| anyhow!("%{id} is not a type"))
    }

    /// Resolves the length of `OpTypeArray %id` (its length operand is a constant id).
    pub fn array_len(&self, len_const: u32) -> Result<u32> {
        match self.constants.get(&len_const).map(|c| &c.kind) {
            Some(ConstKind::Bits32(v)) => Ok(*v),
            Some(ConstKind::Bits64(v)) => Ok(*v as u32),
            _ => bail!("array length %{len_const} is not an integer constant"),
        }
    }

    pub fn has_decoration(&self, id: u32, d: Decoration) -> bool {
        self.decorations.get(&id).map_or(false, |v| v.iter().any(|x| x.decoration == d))
    }

    pub fn decoration_u32(&self, id: u32, d: Decoration) -> Option<u32> {
        self.decorations.get(&id)?.iter().find(|x| x.decoration == d).and_then(|x| match x.operands.first() {
            Some(Operand::LiteralBit32(v)) => Some(*v),
            _ => None,
        })
    }

    pub fn builtin_of(&self, id: u32) -> Option<BuiltIn> {
        self.decorations.get(&id)?.iter().find(|x| x.decoration == Decoration::BuiltIn).and_then(|x| match x.operands.first() {
            Some(Operand::BuiltIn(b)) => Some(*b),
            _ => None,
        })
    }

    pub fn member_decoration_u32(&self, st: u32, member: u32, d: Decoration) -> Option<u32> {
        self.member_decorations.get(&(st, member))?.iter().find(|x| x.decoration == d).and_then(|x| match x.operands.first() {
            Some(Operand::LiteralBit32(v)) => Some(*v),
            _ => None,
        })
    }

    pub fn member_builtin(&self, st: u32, member: u32) -> Option<BuiltIn> {
        self.member_decorations.get(&(st, member))?.iter().find(|x| x.decoration == Decoration::BuiltIn).and_then(|x| {
            match x.operands.first() {
                Some(Operand::BuiltIn(b)) => Some(*b),
                _ => None,
            }
        })
    }

    pub fn member_name(&self, st: u32, member: u32) -> Option<&str> {
        self.member_names.get(&(st, member)).map(|s| s.as_str())
    }

    pub fn name(&self, id: u32) -> Option<&str> {
        self.names.get(&id).map(|s| s.as_str())
    }

    /// The first entry point, or an error.
    pub fn entry(&self) -> Result<&EntryPoint> {
        self.entry_points.first().ok_or_else(|| anyhow!("module has no OpEntryPoint"))
    }

    pub fn function(&self, id: u32) -> Result<(&FunctionInfo, &dr::Function)> {
        let fi = *self.function_index.get(&id).ok_or_else(|| anyhow!("%{id} is not a function"))?;
        Ok((&self.functions[fi], &self.module.functions[fi]))
    }

    /// GLSL-style spelling of a type for humans (`vec4`, `mat3x4`, `float[4]`, `struct Params`).
    pub fn type_name(&self, id: u32) -> String {
        match self.types.get(&id) {
            None => format!("%{id}"),
            Some(Type::Void) => "void".into(),
            Some(Type::Bool) => "bool".into(),
            Some(Type::Int { width: 32, signed: true }) => "int".into(),
            Some(Type::Int { width: 32, signed: false }) => "uint".into(),
            Some(Type::Int { width, signed }) => format!("{}int{width}", if *signed { "" } else { "u" }),
            Some(Type::Float { width: 32 }) => "float".into(),
            Some(Type::Float { width: 64 }) => "double".into(),
            Some(Type::Float { width }) => format!("float{width}"),
            Some(Type::Vector { elem, count }) => {
                let prefix = match self.types.get(elem) {
                    Some(Type::Bool) => "b",
                    Some(Type::Int { signed: true, .. }) => "i",
                    Some(Type::Int { signed: false, .. }) => "u",
                    Some(Type::Float { width: 64 }) => "d",
                    Some(Type::Float { width: 16 }) => "f16",
                    _ => "",
                };
                format!("{prefix}vec{count}")
            }
            Some(Type::Matrix { column, columns }) => match self.types.get(column) {
                Some(Type::Vector { count, .. }) if count == columns => format!("mat{columns}"),
                Some(Type::Vector { count, .. }) => format!("mat{columns}x{count}"),
                _ => format!("mat{columns}x?"),
            },
            Some(Type::Array { elem, len }) => {
                format!("{}[{}]", self.type_name(*elem), self.array_len(*len).map(|n| n.to_string()).unwrap_or("?".into()))
            }
            Some(Type::RuntimeArray { elem }) => format!("{}[]", self.type_name(*elem)),
            Some(Type::Struct { .. }) => format!("struct {}", self.names.get(&id).cloned().unwrap_or_else(|| format!("%{id}"))),
            Some(Type::Pointer { storage, pointee }) => format!("{:?}* {}", storage, self.type_name(*pointee)),
            Some(Type::Image { dim, sampled_type, .. }) => format!("{}image{:?}", self.sampled_prefix(*sampled_type), dim),
            Some(Type::Sampler) => "sampler".into(),
            Some(Type::SampledImage { image }) => match self.types.get(image) {
                Some(Type::Image { dim, sampled_type, arrayed, .. }) => format!(
                    "{}sampler{}{}",
                    self.sampled_prefix(*sampled_type),
                    match dim {
                        spirv::Dim::Dim1D => "1D",
                        spirv::Dim::Dim2D => "2D",
                        spirv::Dim::Dim3D => "3D",
                        spirv::Dim::DimCube => "Cube",
                        other => return format!("sampler{other:?}"),
                    },
                    if *arrayed != 0 { "Array" } else { "" }
                ),
                _ => "sampler?".into(),
            },
            Some(Type::Function { ret, params }) => {
                format!("{}({})", self.type_name(*ret), params.iter().map(|p| self.type_name(*p)).collect::<Vec<_>>().join(", "))
            }
            Some(Type::Other(op)) => format!("{op:?}"),
        }
    }

    fn sampled_prefix(&self, sampled_type: u32) -> &'static str {
        match self.types.get(&sampled_type) {
            Some(Type::Int { signed: true, .. }) => "i",
            Some(Type::Int { signed: false, .. }) => "u",
            _ => "",
        }
    }

    /// Opcode histogram over all function bodies (and `GLSL.std.450` ext-inst names as
    /// `ExtInst:Name`), sorted by descending count then name.
    pub fn opcode_histogram(&self) -> Vec<(String, usize)> {
        let mut h: BTreeMap<String, usize> = BTreeMap::new();
        for f in &self.module.functions {
            for b in &f.blocks {
                for inst in &b.instructions {
                    let key = if inst.class.opcode == Op::ExtInst {
                        let set = inst.operands.first().and_then(|o| o.id_ref_any()).unwrap_or(0);
                        let n = match inst.operands.get(1) {
                            Some(Operand::LiteralExtInstInteger(n)) => *n,
                            _ => 0,
                        };
                        let set_name = self.ext_inst_imports.get(&set).map(|s| s.as_str()).unwrap_or("?");
                        if set_name == "GLSL.std.450" {
                            match spirv::GLOp::from_u32(n) {
                                Some(g) => format!("ExtInst:{g:?}"),
                                None => format!("ExtInst:GLSL.std.450#{n}"),
                            }
                        } else {
                            format!("ExtInst:{set_name}#{n}")
                        }
                    } else {
                        format!("Op{}", inst.class.opname)
                    };
                    *h.entry(key).or_default() += 1;
                }
            }
        }
        let mut v: Vec<(String, usize)> = h.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        v
    }
}

/// Header words of the input and the re-assembled output, plus whether the bodies match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoundTrip {
    pub header_in: Vec<u32>,
    pub header_out: Vec<u32>,
    pub body_identical: bool,
    pub first_diff_word: Option<usize>,
    pub words_in: usize,
    pub words_out: usize,
}

impl RoundTrip {
    /// Human-readable header diff, one line per differing header word.
    pub fn header_diff(&self) -> Vec<String> {
        const NAMES: [&str; 5] = ["magic", "version", "generator", "bound", "schema"];
        let mut out = Vec::new();
        for i in 0..5 {
            let a = self.header_in.get(i).copied();
            let b = self.header_out.get(i).copied();
            if a != b {
                out.push(format!(
                    "header word {i} ({}): {} -> {}",
                    NAMES[i],
                    a.map(|x| format!("{x:#x}")).unwrap_or("-".into()),
                    b.map(|x| format!("{x:#x}")).unwrap_or("-".into())
                ));
            }
        }
        out
    }
}

/// Loads and immediately re-assembles `words`; the M1 "faithful lift" gate.
pub fn roundtrip(words: &[u32]) -> Result<(Lifted, Vec<u32>, RoundTrip)> {
    let lifted = Lifted::load(words)?;
    let out = lifted.assemble();
    let body_identical = words.len() >= 5 && out.len() >= 5 && out[5..] == words[5..];
    let first_diff_word = out.iter().zip(words.iter()).position(|(a, b)| a != b).or_else(|| {
        if out.len() != words.len() {
            Some(out.len().min(words.len()))
        } else {
            None
        }
    });
    let rt = RoundTrip {
        header_in: words[..5.min(words.len())].to_vec(),
        header_out: out[..5.min(out.len())].to_vec(),
        body_identical,
        first_diff_word,
        words_in: words.len(),
        words_out: out.len(),
    };
    Ok((lifted, out, rt))
}
