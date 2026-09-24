//! `info` on the fixture: through the library and through the binary.

mod common;

use shader_ir::lift::{Lifted, Type};
use spirv::{ExecutionModel, StorageClass};
use std::path::Path;
use std::process::Command;

#[test]
fn fixture_info_tables() {
    let Some(words) = common::compile_file(Path::new(common::FIXTURE), false) else { return };
    let l = Lifted::load(&words).unwrap();
    let ep = l.entry().unwrap();
    assert_eq!(ep.model, ExecutionModel::Fragment);
    assert_eq!(ep.name, "main");
    assert_eq!(l.functions.len(), 2, "main and aces");
    let main = l.functions.iter().find(|f| f.id == ep.function).unwrap();
    assert_eq!(main.name.as_deref(), Some("main"));
    assert_eq!(main.headers.values().filter(|h| h.continue_target.is_some()).count(), 1, "one loop");
    assert_eq!(main.headers.values().filter(|h| h.continue_target.is_none()).count(), 1, "one if");

    let by_name = |n: &str| l.variables.iter().find(|v| v.name.as_deref() == Some(n)).unwrap_or_else(|| panic!("no variable {n}"));
    let uv = by_name("uv");
    assert_eq!((uv.storage, uv.location), (StorageClass::Input, Some(0)));
    assert_eq!(l.type_name(uv.pointee), "vec2");
    let o = by_name("o");
    assert_eq!((o.storage, o.location), (StorageClass::Output, Some(0)));
    let tex = by_name("tex");
    assert_eq!((tex.storage, tex.descriptor_set, tex.binding), (StorageClass::UniformConstant, Some(0), Some(0)));
    assert_eq!(l.type_name(tex.pointee), "sampler2D");
    let p = by_name("p");
    assert_eq!((p.storage, p.descriptor_set, p.binding, p.block), (StorageClass::Uniform, Some(0), Some(1), true));
    let Type::Struct { members } = l.ty(p.pointee).unwrap() else { panic!("Params is not a struct") };
    assert_eq!(members.len(), 3);
    assert_eq!(l.name(p.pointee), Some("Params"));
    let layout: Vec<(String, String, u32)> = (0..3)
        .map(|m| {
            (
                l.member_name(p.pointee, m).unwrap().to_string(),
                l.type_name(members[m as usize]),
                l.member_decoration_u32(p.pointee, m, spirv::Decoration::Offset).unwrap(),
            )
        })
        .collect();
    assert_eq!(
        layout,
        vec![("exposure".into(), "float".into(), 0), ("gamma".into(), "float".into(), 4), ("texel".into(), "vec2".into(), 8)]
    );

    // Def-use: the uv variable is used by loads inside main.
    assert!(l.defs.contains_key(&uv.id));
    assert!(l.uses.get(&uv.id).map_or(false, |u| !u.is_empty()));
    // Every result id in a function body has a def site and a result type.
    for f in &l.module.functions {
        for b in &f.blocks {
            for inst in &b.instructions {
                if let Some(id) = inst.result_id {
                    assert!(l.defs.contains_key(&id), "%{id} has no def site");
                    assert_eq!(l.result_types.get(&id), inst.result_type.as_ref());
                }
            }
        }
    }
    let hist = l.opcode_histogram();
    let count = |n: &str| hist.iter().find(|(k, _)| k == n).map(|(_, c)| *c).unwrap_or(0);
    assert_eq!(count("OpKill"), 1);
    assert_eq!(count("OpImageSampleImplicitLod"), 2);
    assert_eq!(count("ExtInst:Pow"), 1);
    assert_eq!(count("ExtInst:FMix"), 1);
    assert_eq!(count("ExtInst:FClamp"), 1);
    assert_eq!(count("OpFunctionCall"), 1);
    assert!(hist.windows(2).all(|w| w[0].1 >= w[1].1), "sorted by count");
}

#[test]
fn info_and_roundtrip_via_binary() {
    let Some(words) = common::compile_file(Path::new(common::FIXTURE), false) else { return };
    let spv = common::tmp_dir().join("info_fixture.spv");
    std::fs::write(&spv, shader_ir::bytes_from_words(&words)).unwrap();
    let bin = env!("CARGO_BIN_EXE_shader-ir");
    let out = Command::new(bin).arg("info").arg(&spv).output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    for needle in ["entry point: \"main\" Fragment", "tex", "sampler2D set=0 binding=0", "Input vec2 location=0", "member 2 texel", "offset=8", "opcode histogram:", "ExtInst:Pow"] {
        assert!(text.contains(needle), "info output lacks {needle:?}:\n{text}");
    }
    let rt = common::tmp_dir().join("info_fixture_rt.spv");
    let out = Command::new(bin).arg("roundtrip").arg(&spv).arg("--out").arg(&rt).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{text}");
    assert!(text.contains("body_identical=true"), "{text}");
    assert!(text.contains("header word 2 (generator): 0x8000b -> 0xf0000"), "{text}");
    let back = shader_ir::read_spv(&rt).unwrap();
    assert_eq!(back[5..], words[5..]);
}
