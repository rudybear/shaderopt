//! M3 precision-demotion tests (`shader-ir demote`): `relaxed` decorations, explicit f16 on
//! single sites, chains, phis, GLSL.std.450 sites and constants, the rejection messages, and
//! the whole corpus demoted at once, each checked with spirv-val and against the interpreter's
//! `--f16-sites` prediction.

mod common;

use common::{compile_src, Run, GLSLANG, PRELUDE, SPIRV_VAL};
use shader_ir::interp::{EvalConfig, EvalOutput, Filter, Mode};
use shader_ir::lift::{Lifted, Type};
use shader_ir::npy::Image;
use shader_ir::passes::demote::{self, DemoteMode, Report};
use shader_ir::passes::{self, Class, EditOp};
use spirv::{Op, StorageClass};
use std::path::Path;

const SPV_DIR: &str = "/home/rudybear/sources/shaderopt/lab/build/spv";

macro_rules! compile_or_skip {
    ($name:expr, $src:expr) => {
        match compile_src($name, &format!("{PRELUDE}{}", $src), false) {
            Some(w) => w,
            None => return,
        }
    };
}

/// `glslang -V -Os` (spirv-opt size passes: mem2reg turns locals into phis).
fn compile_os(name: &str, src: &str) -> Option<Vec<u32>> {
    if !common::glslang_available() {
        return None;
    }
    let d = common::tmp_dir();
    let frag = d.join(format!("{name}.frag"));
    let out = d.join(format!("{name}_os.spv"));
    std::fs::write(&frag, format!("{PRELUDE}{src}")).unwrap();
    let st = std::process::Command::new(GLSLANG).args(["-V", "-Os", "-o"]).arg(&out).arg(&frag).output().expect("run glslang");
    assert!(st.status.success(), "glslang -Os failed: {}{}", String::from_utf8_lossy(&st.stdout), String::from_utf8_lossy(&st.stderr));
    Some(shader_ir::read_spv(&out).unwrap())
}

fn validate_with_tool(words: &[u32]) {
    if !Path::new(SPIRV_VAL).is_file() {
        return;
    }
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let p = common::tmp_dir().join(format!("dm_val_{}_{}.spv", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    std::fs::write(&p, shader_ir::bytes_from_words(words)).unwrap();
    let st = std::process::Command::new(SPIRV_VAL).arg(&p).output().expect("run spirv-val");
    assert!(st.status.success(), "spirv-val rejected the demoted module: {}", String::from_utf8_lossy(&st.stderr));
}

fn demote(words: &[u32], sites: &[u32], mode: DemoteMode, group: bool) -> (Vec<u32>, Vec<EditOp>, Report) {
    let mut l = Lifted::load(words).unwrap();
    let mut ops = Vec::new();
    let report = demote::run(&mut l, sites, mode, group, &mut ops).unwrap_or_else(|e| panic!("demote: {e:#}"));
    let out = l.assemble();
    passes::validate(&out).unwrap_or_else(|e| panic!("in-process validation failed: {e:#}"));
    validate_with_tool(&out);
    (out, ops, report)
}

fn demote_err(words: &[u32], sites: &[u32], mode: DemoteMode) -> String {
    let mut l = Lifted::load(words).unwrap();
    let mut ops = Vec::new();
    format!("{:#}", demote::run(&mut l, sites, mode, false, &mut ops).expect_err("expected a rejection"))
}

fn histogram(words: &[u32], key: &str) -> usize {
    Lifted::load(words).unwrap().opcode_histogram().iter().find(|(n, _)| n == key).map(|x| x.1).unwrap_or(0)
}

fn has_float16_capability(words: &[u32]) -> bool {
    Lifted::load(words).unwrap().module.capabilities.iter().any(|c| c.operands.first() == Some(&rspirv::dr::Operand::Capability(spirv::Capability::Float16)))
}

/// Result ids of the instructions with the given opcode (or `ExtInst:Name`), in layout order.
fn ids_of(words: &[u32], key: &str) -> Vec<u32> {
    let l = Lifted::load(words).unwrap();
    let mut v = Vec::new();
    for f in &l.module.functions {
        for b in &f.blocks {
            for inst in &b.instructions {
                let k = match passes::glsl_op(&l, inst) {
                    Some(g) => format!("ExtInst:{g:?}"),
                    None => format!("Op{}", inst.class.opname),
                };
                if k == key {
                    if let Some(r) = inst.result_id {
                        v.push(r);
                    }
                }
            }
        }
    }
    v
}

/// Converts f16 -> f32 and f32 -> f16 in the module, by direction.
fn converts(words: &[u32]) -> (usize, usize) {
    let l = Lifted::load(words).unwrap();
    let (mut to16, mut to32) = (0, 0);
    for f in &l.module.functions {
        for b in &f.blocks {
            for inst in &b.instructions {
                if inst.class.opcode == Op::FConvert {
                    match passes::float_width(&l, inst.result_type.unwrap()) {
                        Some(16) => to16 += 1,
                        Some(32) => to32 += 1,
                        _ => {}
                    }
                }
            }
        }
    }
    (to16, to32)
}

fn f32_run(w: usize, h: usize, f16_sites: &[u32]) -> Run {
    let mut r = Run::new(w, h, Mode::F32);
    r.cfg.f16_sites = f16_sites.to_vec();
    r
}

fn assert_bit_identical(a: &EvalOutput, b: &EvalOutput, what: &str) {
    assert_eq!(a.discarded_pixels, b.discarded_pixels, "{what}: discard count");
    for (loc, ia) in &a.outputs {
        let ib = &b.outputs[loc];
        for y in 0..ia.height {
            for x in 0..ia.width {
                let (ta, tb) = (ia.texel(x, y), ib.texel(x, y));
                for c in 0..4 {
                    assert_eq!(ta[c].to_bits(), tb[c].to_bits(), "{what}: pixel ({x},{y}) channel {c}: {} vs {}", ta[c], tb[c]);
                }
            }
        }
    }
}

/// `|a - b|` in units of the f16 spacing at the larger magnitude (f16 ULP); NaN vs NaN is 0.
fn f16_ulp_distance(a: f32, b: f32) -> f64 {
    if a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan()) {
        return 0.0;
    }
    if a.is_nan() || b.is_nan() || a.is_infinite() != b.is_infinite() {
        return f64::INFINITY;
    }
    let m = a.abs().max(b.abs()) as f64;
    let ulp = if m < 2f64.powi(-14) { 2f64.powi(-24) } else { 2f64.powi(m.log2().floor() as i32 - 10) };
    ((a as f64) - (b as f64)).abs() / ulp
}

fn worst_f16_ulp(a: &EvalOutput, b: &EvalOutput) -> (f64, String) {
    let mut worst = (0.0f64, String::new());
    for (loc, ia) in &a.outputs {
        let ib = &b.outputs[loc];
        for y in 0..ia.height {
            for x in 0..ia.width {
                let (ta, tb) = (ia.texel(x, y), ib.texel(x, y));
                for c in 0..4 {
                    let d = f16_ulp_distance(ta[c], tb[c]);
                    if d > worst.0 {
                        worst = (d, format!("location {loc} pixel ({x},{y}) channel {c}: {} vs {}", ta[c], tb[c]));
                    }
                }
            }
        }
    }
    worst
}

// ---- (1) relaxed ------------------------------------------------------------------------------

#[test]
fn relaxed_adds_exactly_n_decorations() {
    let w = compile_or_skip!("dm_relaxed", "void main() { float a = uv.x * 2.0; float b = exp(a) + uv.y; o = vec4(a, b, a * b, 1.0); }");
    let mut sites = ids_of(&w, "OpFMul");
    sites.extend(ids_of(&w, "ExtInst:Exp"));
    sites.extend(ids_of(&w, "OpFAdd"));
    assert_eq!(sites.len(), 4, "{sites:?}");
    let (out, ops, report) = demote(&w, &sites, DemoteMode::Relaxed, false);
    assert_eq!(report.demoted, 4);
    assert_eq!(ops.len(), 4);
    for o in &ops {
        assert_eq!(o.pass, "demote_relaxed");
        assert_eq!(o.class, Class::Lossy);
        assert_eq!(o.replaced_by, Some(o.target));
        assert!(o.detail.contains("RelaxedPrecision"), "{}", o.detail);
    }
    let l0 = Lifted::load(&w).unwrap();
    let l1 = Lifted::load(&out).unwrap();
    let relaxed = |l: &Lifted| l.module.annotations.iter().filter(|a| a.operands.get(1) == Some(&rspirv::dr::Operand::Decoration(spirv::Decoration::RelaxedPrecision))).count();
    assert_eq!(relaxed(&l1), relaxed(&l0) + 4);
    for id in &sites {
        assert!(l1.has_decoration(*id, spirv::Decoration::RelaxedPrecision));
    }
    // Nothing else changed: same instruction count, same bound, the body is otherwise identical.
    assert_eq!(passes::instruction_count(&l0), passes::instruction_count(&l1));
    assert_eq!(l0.bound, l1.bound);
    assert_eq!(l0.module.types_global_values.len(), l1.module.types_global_values.len());
    // RelaxedPrecision is a hint: the interpreter ignores it.
    let a = f32_run(16, 16, &[]).eval_ok(&w);
    let b = f32_run(16, 16, &[]).eval_ok(&out);
    assert_bit_identical(&a, &b, "relaxed");
    // Listing a site twice decorates once; an already decorated site is skipped.
    let (_, ops2, report2) = demote(&out, &[sites[0], sites[0]], DemoteMode::Relaxed, false);
    assert_eq!(ops2.len(), 0);
    assert_eq!(report2.skipped.len(), 1);
}

// ---- (2) f16 on a single OpFMul ----------------------------------------------------------------

#[test]
fn f16_single_fmul() {
    // uv at 16x16 is (2k+1)/32: exactly representable in f16, so rounding the operand at the
    // region entry changes nothing and the demoted module must equal the --f16-sites prediction.
    let w = compile_or_skip!("dm_fmul", "void main() { o = vec4(uv.x * uv.y); }");
    let sites = ids_of(&w, "OpFMul");
    assert_eq!(sites.len(), 1);
    let (out, ops, report) = demote(&w, &sites, DemoteMode::F16, false);
    assert!(has_float16_capability(&out));
    assert!(!has_float16_capability(&w));
    assert_eq!(ops.len(), 1, "{ops:#?}");
    assert_eq!(ops[0].pass, "demote_f16");
    assert_eq!(ops[0].class, Class::Lossy);
    assert_eq!(ops[0].target, sites[0]);
    assert_eq!(ops[0].replaced_by, Some(sites[0]));
    assert!(ops[0].detail.starts_with("OpFMul") && ops[0].detail.contains("f32->f16 (+3 converts)"), "{}", ops[0].detail);
    assert_eq!((report.converts_in, report.converts_out), (2, 1));
    assert_eq!(converts(&out), (2, 1));
    // The site keeps its id and now has an f16 result type.
    let l = Lifted::load(&out).unwrap();
    assert_eq!(passes::float_width(&l, l.result_types[&sites[0]]), Some(16));
    assert_eq!(l.type_name(l.result_types[&sites[0]]), "float16");
    let a = f32_run(16, 16, &sites).eval_ok(&w);
    let b = f32_run(16, 16, &[]).eval_ok(&out);
    assert_bit_identical(&a, &b, "f16 single fmul");
    // And the demotion did change the value: f16 rounding of uv.x*uv.y differs from f32.
    let plain = f32_run(16, 16, &[]).eval_ok(&w);
    let differs = (0..16).flat_map(|y| (0..16).map(move |x| (x, y))).any(|(x, y)| plain.outputs[&0].texel(x, y)[0].to_bits() != b.outputs[&0].texel(x, y)[0].to_bits());
    assert!(differs, "the f16 product never differs from f32?");
}

#[test]
fn f16_single_fmul_with_constant_has_one_convert_each_way() {
    let w = compile_or_skip!("dm_fmul_c", "void main() { o = vec4(uv.x * 3.0); }");
    let sites = ids_of(&w, "OpFMul");
    let (out, _, report) = demote(&w, &sites, DemoteMode::F16, false);
    assert_eq!((report.converts_in, report.converts_out), (1, 1));
    assert_eq!(converts(&out), (1, 1));
    let a = f32_run(16, 16, &sites).eval_ok(&w);
    let b = f32_run(16, 16, &[]).eval_ok(&out);
    assert_bit_identical(&a, &b, "f16 fmul const");
}

// ---- (3) chain with --group-converts -----------------------------------------------------------

#[test]
fn f16_chain_grouped() {
    let w = compile_or_skip!("dm_chain", "void main() { o = vec4((uv.x * 2.0) * 3.0 + 1.0); }");
    let mut sites = ids_of(&w, "OpFMul");
    sites.extend(ids_of(&w, "OpFAdd"));
    assert_eq!(sites.len(), 3, "{sites:?}");
    // All at once: the chain shares one convert in and one out; nothing to group.
    let (out, ops, report) = demote(&w, &sites, DemoteMode::F16, true);
    assert_eq!(ops.iter().filter(|o| o.pass == "demote_f16").count(), 3);
    assert_eq!(report.converts_removed, 0);
    assert_eq!(converts(&out), (1, 1));
    let a = f32_run(16, 16, &sites).eval_ok(&w);
    let b = f32_run(16, 16, &[]).eval_ok(&out);
    assert_bit_identical(&a, &b, "f16 chain");
    // Incrementally (one site per call on the previous output): each step first sees the
    // convert back of the previous site as its operand, --group-converts removes the pair.
    let mut cur = w.clone();
    let mut removed = 0;
    for (k, s) in sites.iter().enumerate() {
        let (next, ops, report) = demote(&cur, &[*s], DemoteMode::F16, true);
        if k > 0 {
            assert_eq!(report.converts_removed, 1, "step {k}: {ops:#?}");
            let g: Vec<&EditOp> = ops.iter().filter(|o| o.pass == "group_converts").collect();
            assert_eq!(g.len(), 1);
            assert_eq!(g[0].class, Class::Exact);
            assert!(g[0].detail.contains("float16 -> float"), "{}", g[0].detail);
        }
        removed += report.converts_removed;
        cur = next;
    }
    assert_eq!(removed, 2);
    assert_eq!(converts(&cur), (1, 1), "incremental chain still carries redundant converts");
    let c = f32_run(16, 16, &[]).eval_ok(&cur);
    assert_bit_identical(&a, &c, "f16 chain incremental");
    // The f16 constants are shared (2.0, 3.0, 1.0 once each) and Float16 was added once.
    let l = Lifted::load(&cur).unwrap();
    assert_eq!(l.module.capabilities.iter().filter(|c| c.operands.first() == Some(&rspirv::dr::Operand::Capability(spirv::Capability::Float16))).count(), 1);
    assert_eq!(l.module.types_global_values.iter().filter(|i| i.class.opcode == Op::TypeFloat && i.operands.first() == Some(&rspirv::dr::Operand::LiteralBit32(16))).count(), 1);
}

// ---- (4) phi -----------------------------------------------------------------------------------

#[test]
fn f16_phi_site() {
    let Some(w) = compile_os("dm_phi", "void main() { float s = 0.0; for (int i = 0; i < 3; ++i) { s = s * 0.5 + uv.x; } o = vec4(s); }") else { return };
    let l0 = Lifted::load(&w).unwrap();
    let phis: Vec<u32> = ids_of(&w, "OpPhi").into_iter().filter(|id| passes::float_width(&l0, l0.result_types[id]) == Some(32)).collect();
    assert_eq!(phis.len(), 1, "expected one float phi from -Os: {phis:?}");
    let mut sites = phis.clone();
    sites.extend(ids_of(&w, "OpFMul"));
    sites.extend(ids_of(&w, "OpFAdd"));
    assert_eq!(sites.len(), 3);
    let (out, ops, report) = demote(&w, &sites, DemoteMode::F16, true);
    assert_eq!(ops.len(), 3, "{ops:#?}");
    // Inputs: the constant 0.0 becomes an f16 constant, uv.x converts once (in the FAdd);
    // the phi leaves the set once (the OpCompositeConstruct of the output).
    assert_eq!((report.converts_in, report.converts_out), (1, 1));
    let l = Lifted::load(&out).unwrap();
    let phi = passes::def_inst(&l, phis[0]).unwrap();
    assert_eq!(passes::float_width(&l, phi.result_type.unwrap()), Some(16));
    // The convert back sits after the phis of the block, before anything else.
    let (fi, bi, ii) = match l.defs[&phis[0]] {
        shader_ir::lift::Site::Inst(f, b, i) => (f, b, i),
        s => panic!("{s:?}"),
    };
    let block = &l.module.functions[fi].blocks[bi];
    let first_non_phi = block.instructions.iter().position(|i| i.class.opcode != Op::Phi).unwrap();
    assert!(ii < first_non_phi);
    let conv = &block.instructions[first_non_phi];
    assert_eq!(conv.class.opcode, Op::FConvert);
    assert_eq!(passes::id_op(conv, 0), Some(phis[0]));
    let a = f32_run(16, 16, &sites).eval_ok(&w);
    let b = f32_run(16, 16, &[]).eval_ok(&out);
    assert_bit_identical(&a, &b, "f16 phi");
    // Demoting only the phi: its incoming FAdd value converts at the end of the loop body
    // (the predecessor block), the constant becomes f16, and the two users convert back once.
    let (out2, _, report2) = demote(&w, &phis, DemoteMode::F16, false);
    assert_eq!((report2.converts_in, report2.converts_out), (1, 1));
    let l2 = Lifted::load(&out2).unwrap();
    let phi2 = passes::def_inst(&l2, phis[0]).unwrap();
    for pair in phi2.operands.chunks(2) {
        let v = pair[0].id_ref_any().unwrap();
        assert_eq!(passes::float_width(&l2, l2.result_types[&v]), Some(16), "phi operand %{v} is not f16");
    }
    let c = f32_run(16, 16, &phis).eval_ok(&w);
    let d = f32_run(16, 16, &[]).eval_ok(&out2);
    assert_bit_identical(&c, &d, "f16 phi only");
}

// ---- (5) GLSL.std.450 site and an f16 constant -------------------------------------------------

#[test]
fn f16_ext_inst_and_constant() {
    let w = compile_or_skip!("dm_exp", "void main() { o = vec4(exp(uv.x) + 0.5); }");
    let mut sites = ids_of(&w, "ExtInst:Exp");
    sites.extend(ids_of(&w, "OpFAdd"));
    assert_eq!(sites.len(), 2);
    let (out, ops, report) = demote(&w, &sites, DemoteMode::F16, false);
    assert!(ops[0].detail.starts_with("OpExtInst Exp"), "{}", ops[0].detail);
    assert_eq!((report.converts_in, report.converts_out), (1, 1));
    let l = Lifted::load(&out).unwrap();
    // An OpConstant of the f16 type with the half bits of 0.5 (0x3800), referenced by the FAdd.
    let f16_ty = l.types.iter().find(|(_, t)| **t == Type::Float { width: 16 }).map(|(id, _)| *id).expect("f16 type");
    let half_consts: Vec<(u32, u32)> = l
        .constants
        .iter()
        .filter(|(_, c)| c.ty == f16_ty)
        .filter_map(|(id, c)| match c.kind {
            shader_ir::lift::ConstKind::Bits32(b) => Some((*id, b)),
            _ => None,
        })
        .collect();
    assert_eq!(half_consts.len(), 1, "{half_consts:?}");
    assert_eq!(half_consts[0].1, 0x3800);
    let fadd = passes::def_inst(&l, sites[1]).unwrap();
    assert!(fadd.operands.iter().any(|o| o.id_ref_any() == Some(half_consts[0].0)), "FAdd does not use the f16 constant");
    assert_eq!(passes::float_width(&l, l.result_types[&sites[0]]), Some(16));
    let a = f32_run(16, 16, &sites).eval_ok(&w);
    let b = f32_run(16, 16, &[]).eval_ok(&out);
    assert_bit_identical(&a, &b, "f16 exp + const");
    // A vector constant becomes an OpConstantComposite of f16 scalars.
    let w2 = compile_or_skip!("dm_vconst", "void main() { o = vec4(uv * vec2(0.25, 8.0), 0.0, 1.0); }");
    let s2 = ids_of(&w2, "OpFMul");
    let (out2, _, _) = demote(&w2, &s2, DemoteMode::F16, false);
    let l2 = Lifted::load(&out2).unwrap();
    let mul = passes::def_inst(&l2, s2[0]).unwrap();
    let cid = mul.operands.iter().filter_map(|o| o.id_ref_any()).find(|id| l2.constants.contains_key(id)).expect("constant operand");
    assert_eq!(l2.type_name(l2.constants[&cid].ty), "f16vec2");
    let x = f32_run(16, 16, &s2).eval_ok(&w2);
    let y = f32_run(16, 16, &[]).eval_ok(&out2);
    assert_bit_identical(&x, &y, "f16 vector const");
}

// ---- (6) rejections ----------------------------------------------------------------------------

#[test]
fn rejects_comparisons_uniform_loads_and_bad_ids() {
    let w = compile_or_skip!(
        "dm_reject",
        "layout(set = 0, binding = 1) uniform Params { float k; } u;
         layout(set = 0, binding = 0) uniform sampler2D s;
         void main() { float a = u.k * uv.x; if (a < 0.5) discard; o = texture(s, uv) * a + dFdx(uv.x); }"
    );
    let l = Lifted::load(&w).unwrap();
    let cmp = ids_of(&w, "OpFOrdLessThan");
    assert_eq!(cmp.len(), 1);
    let e = demote_err(&w, &cmp, DemoteMode::F16);
    assert!(e.contains(&format!("%{}", cmp[0])) && e.contains("OpFOrdLessThan") && e.contains("comparison"), "{e}");
    // Relaxed rejects it too (not f32-typed).
    let e = demote_err(&w, &cmp, DemoteMode::Relaxed);
    assert!(e.contains("comparison"), "{e}");
    // The uniform load: the f32 load whose pointer roots in the Uniform block.
    let uload = ids_of(&w, "OpLoad")
        .into_iter()
        .find(|id| passes::def_inst(&l, *id).and_then(|i| passes::id_op(i, 0)).and_then(|p| passes::pointer_root(&l, p)).map_or(false, |(_, s)| s == StorageClass::Uniform))
        .expect("uniform load");
    let e = demote_err(&w, &[uload], DemoteMode::F16);
    assert!(e.contains(&format!("%{uload}")) && e.contains("Uniform") && e.contains("interface"), "{e}");
    // An Input load (uv), an image sample, a derivative and a variable with an unlisted load.
    let iload = ids_of(&w, "OpLoad").into_iter().find(|id| passes::def_inst(&l, *id).and_then(|i| passes::id_op(i, 0)).and_then(|p| passes::pointer_root(&l, p)).map_or(false, |(_, s)| s == StorageClass::Input)).unwrap();
    let e = demote_err(&w, &[iload], DemoteMode::F16);
    assert!(e.contains("Input"), "{e}");
    let img = ids_of(&w, "OpImageSampleImplicitLod");
    let e = demote_err(&w, &img, DemoteMode::F16);
    assert!(e.contains("image"), "{e}");
    let d = ids_of(&w, "OpDPdx");
    let e = demote_err(&w, &d, DemoteMode::F16);
    assert!(e.contains("derivative"), "{e}");
    // Every offender is listed in one error.
    let e = demote_err(&w, &[img[0], d[0], uload], DemoteMode::F16);
    assert!(e.contains("3 site(s) rejected") && e.contains("image") && e.contains("derivative") && e.contains("Uniform"), "{e}");
    // Loads of `a` (a Function variable): listing one but not the others is rejected naming
    // the missing ones; a non-existent id and a non-f32 id are reported by the type check.
    let a_loads: Vec<u32> = ids_of(&w, "OpLoad").into_iter().filter(|id| passes::def_inst(&l, *id).and_then(|i| passes::id_op(i, 0)).map_or(false, |p| l.name(p) == Some("a"))).collect();
    assert!(a_loads.len() >= 2, "{a_loads:?}");
    let e = demote_err(&w, &a_loads[..1], DemoteMode::F16);
    assert!(e.contains("\"a\"") && e.contains(&format!("%{}", a_loads[1])) && e.contains("not listed"), "{e}");
    let e = demote_err(&w, &[9999, cmp[0]], DemoteMode::F16);
    assert!(e.contains("%9999 does not exist") && e.contains("comparison"), "{e}");
    let ptr = ids_of(&w, "OpAccessChain")[0];
    let e = demote_err(&w, &[ptr], DemoteMode::F16);
    assert!(e.contains("not f32-typed"), "{e}");
    // A matrix op.
    let w2 = compile_or_skip!("dm_mat", "layout(set = 0, binding = 1) uniform P { mat4 m; } u; void main() { o = u.m * vec4(uv, 0.0, 1.0); }");
    let m = ids_of(&w2, "OpMatrixTimesVector");
    let e = demote_err(&w2, &m, DemoteMode::F16);
    assert!(e.contains("matrix"), "{e}");
    // A mixed-signature GLSL.std.450 instruction.
    let w3 = compile_or_skip!("dm_ldexp", "void main() { o = vec4(ldexp(uv.x, 3)); }");
    let ld = ids_of(&w3, "ExtInst:Ldexp");
    let e = demote_err(&w3, &ld, DemoteMode::F16);
    assert!(e.contains("Ldexp") && e.contains("mixed"), "{e}");
    // prune() drops exactly the rejected ones and keeps the rest.
    let mut all: Vec<u32> = Vec::new();
    for (id, ty) in &l.result_types {
        if passes::float_width(&l, *ty) == Some(32) {
            all.push(*id);
        }
    }
    all.sort();
    let (keep, rejected) = demote::prune(&l, &all);
    assert!(keep.contains(&ids_of(&w, "OpFMul")[0]));
    assert!(rejected.iter().any(|(id, _)| *id == img[0]));
    assert!(rejected.iter().any(|(id, _)| *id == uload));
    assert!(!keep.iter().any(|id| a_loads.contains(id)) || a_loads.iter().all(|id| keep.contains(id)));
    let (out, _, _) = demote(&w, &keep, DemoteMode::F16, true);
    assert!(has_float16_capability(&out));
}

// ---- Function variables through their loads ----------------------------------------------------

#[test]
fn f16_function_variable_through_loads() {
    let w = compile_or_skip!("dm_var", "void main() { float a = uv.x * 2.0; o = vec4(a * 3.0, a, 0.0, 1.0); }");
    let l = Lifted::load(&w).unwrap();
    let a_loads: Vec<u32> = ids_of(&w, "OpLoad").into_iter().filter(|id| passes::def_inst(&l, *id).and_then(|i| passes::id_op(i, 0)).map_or(false, |p| l.name(p) == Some("a"))).collect();
    assert_eq!(a_loads.len(), 2);
    let mut sites = ids_of(&w, "OpFMul");
    sites.extend(&a_loads);
    let (out, ops, report) = demote(&w, &sites, DemoteMode::F16, true);
    // Two FMul + two loads listed; the variable and its store are extra records.
    assert_eq!(report.demoted, 4);
    assert!(ops.iter().any(|o| o.detail.starts_with("OpVariable") && o.detail.contains("\"a\"") && o.detail.contains("f32->f16")), "{ops:#?}");
    assert!(ops.iter().any(|o| o.detail.starts_with("OpStore") && o.detail.contains("(+0 converts)")), "{ops:#?}");
    // uv.x converts in once; the second load leaves the set (vec4 construct) once, the
    // second FMul once: no convert sits between the store and the loads.
    assert_eq!((report.converts_in, report.converts_out), (1, 2));
    let l1 = Lifted::load(&out).unwrap();
    let var = l1.names.iter().find(|(_, n)| n.as_str() == "a").map(|(id, _)| *id).unwrap();
    assert_eq!(l1.type_name(l1.result_types[&var]), "Function* float16");
    let a = f32_run(16, 16, &sites).eval_ok(&w);
    let b = f32_run(16, 16, &[]).eval_ok(&out);
    assert_bit_identical(&a, &b, "f16 variable");
}

// ---- (7) corpus --------------------------------------------------------------------------------

fn gradient(w: usize, h: usize) -> Image {
    let mut img = Image::new(w, h);
    for y in 0..h {
        for x in 0..w {
            img.set_texel(x, y, [x as f32 / (w - 1).max(1) as f32, y as f32 / (h - 1).max(1) as f32, 0.5, 1.0]);
        }
    }
    img
}

fn leaf_count(l: &Lifted, ty: u32) -> usize {
    match l.ty(ty).unwrap() {
        Type::Vector { count, .. } => *count as usize,
        Type::Matrix { column, columns } => *columns as usize * leaf_count(l, *column),
        Type::Array { elem, len } => l.array_len(*len).unwrap() as usize * leaf_count(l, *elem),
        Type::Struct { members } => members.iter().map(|m| leaf_count(l, *m)).sum(),
        _ => 1,
    }
}

/// Generic inputs from reflection (as the rewrite corpus tests): a gradient image per sampler,
/// ones for every uniform member (zero for `pad*`).
fn corpus_cfg(l: &Lifted, w: usize, h: usize, f16_sites: &[u32]) -> EvalConfig {
    let mut r = f32_run(w, h, f16_sites);
    for v in &l.variables {
        match v.storage {
            StorageClass::UniformConstant => {
                let name = v.name.clone().unwrap_or_else(|| format!("binding{}", v.binding.unwrap_or(0)));
                r = r.sampler(&name, gradient(32, 32), Filter::Linear);
            }
            StorageClass::Uniform | StorageClass::PushConstant => {
                if let Type::Struct { members } = l.ty(v.pointee).unwrap() {
                    for (m, mty) in members.iter().enumerate() {
                        let name = l.member_name(v.pointee, m as u32).map(String::from).unwrap_or_else(|| format!("member{m}"));
                        let fill = if name.starts_with("pad") { "0" } else { "1" };
                        let n = leaf_count(l, *mty);
                        r = r.uniform(&name, &format!("[{}]", vec![fill; n].join(",")));
                    }
                }
            }
            _ => {}
        }
    }
    r.cfg
}

fn corpus_modules() -> Vec<std::path::PathBuf> {
    let dir = Path::new(SPV_DIR);
    if !dir.is_dir() {
        eprintln!("SKIP: no corpus at {SPV_DIR}");
        return vec![];
    }
    let mut v: Vec<_> = std::fs::read_dir(dir).unwrap().filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().map_or(false, |e| e == "spv")).collect();
    v.sort();
    v
}

/// Candidate ids from `shader-ir analyze`: f32 float-typed sites with no sinks.
fn analyze_candidates(spv: &Path) -> Vec<u32> {
    let bin = env!("CARGO_BIN_EXE_shader-ir");
    let out = common::tmp_dir().join(format!("{}.analysis.json", spv.file_name().unwrap().to_string_lossy()));
    let st = std::process::Command::new(bin).args(["analyze", "--spv"]).arg(spv).arg("--out").arg(&out).output().expect("run shader-ir analyze");
    assert!(st.status.success(), "analyze failed: {}", String::from_utf8_lossy(&st.stderr));
    let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    let l = Lifted::load(&shader_ir::read_spv(spv).unwrap()).unwrap();
    let mut ids = Vec::new();
    for e in json["instructions"].as_array().unwrap() {
        let id = e["id"].as_u64().unwrap() as u32;
        if id == 0 || !e["sinks"].as_array().unwrap().is_empty() {
            continue;
        }
        if l.result_types.get(&id).map_or(false, |t| passes::float_width(&l, *t) == Some(32)) {
            ids.push(id);
        }
    }
    ids
}

/// The largest deviation observed between the demoted corpus module and the `--f16-sites`
/// prediction, in f16 ULPs: operands entering a demoted region are rounded to f16 by the
/// demoted module but not by the prediction (which rounds results only), and constants are
/// f16 constants in the demoted module; the error of one operation on operands each
/// perturbed by half an f16 ULP is up to about one f16 ULP of the result, and the corpus
/// shaders chain such operations. Measured 2025-09 on the 9 corpus shaders (see the test
/// output for the per-shader value).
const CORPUS_MAX_F16_ULP: f64 = 8.0;

#[test]
fn corpus_demote_all_candidates_f16() {
    let mut summary = Vec::new();
    for p in corpus_modules() {
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        let words = shader_ir::read_spv(&p).unwrap();
        let l0 = Lifted::load(&words).unwrap();
        let candidates = analyze_candidates(&p);
        let (sites, rejected) = demote::prune(&l0, &candidates);
        let (out, ops, report) = demote(&words, &sites, DemoteMode::F16, true);
        assert!(has_float16_capability(&out) || sites.is_empty());
        let l1 = Lifted::load(&out).unwrap();
        // Every listed id keeps its id and now has an f16 result.
        for id in &sites {
            assert_eq!(passes::float_width(&l1, l1.result_types[id]), Some(16), "{name}: %{id} is not f16 after demotion");
        }
        let n_demote = ops.iter().filter(|o| o.pass == "demote_f16").count();
        assert!(ops.iter().filter(|o| o.pass == "demote_f16").all(|o| o.class == Class::Lossy));
        assert!(ops.iter().filter(|o| o.pass == "group_converts").all(|o| o.class == Class::Exact));
        let (to16, to32) = converts(&out);
        // Interpreter: the demoted module in f32 mode (f16 arithmetic where the types say so)
        // against the original with every demoted site rounded to f16.
        let cfg_a = corpus_cfg(&l0, 64, 36, &sites);
        let cfg_b = corpus_cfg(&l0, 64, 36, &[]);
        let a = shader_ir::interp::evaluate(&l0, &cfg_a).unwrap_or_else(|e| panic!("{name}: eval original: {e:#}"));
        let b = shader_ir::interp::evaluate(&l1, &cfg_b).unwrap_or_else(|e| panic!("{name}: eval demoted: {e:#}"));
        assert_eq!(a.discarded_pixels, b.discarded_pixels, "{name}: discard count");
        let (worst, at) = worst_f16_ulp(&a, &b);
        let line = format!(
            "{name}: candidates {} -> demoted {} sites ({} rejected by kind), {} demote ops, converts in {} / out {} / removed {} (module: {} to f16, {} to f32), worst {worst:.2} f16 ULP vs --f16-sites{}",
            candidates.len(),
            sites.len(),
            rejected.len(),
            n_demote,
            report.converts_in,
            report.converts_out,
            report.converts_removed,
            to16,
            to32,
            if at.is_empty() { String::new() } else { format!(" at {at}") }
        );
        eprintln!("{line}");
        let mut kinds: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
        for (_, why) in &rejected {
            *kinds.entry(why.split(':').next().unwrap_or(why).to_string()).or_default() += 1;
        }
        eprintln!("    rejected: {kinds:?}");
        assert!(worst <= CORPUS_MAX_F16_ULP, "{line}");
        summary.push(line);
    }
    eprintln!("corpus summary:\n  {}", summary.join("\n  "));
}
