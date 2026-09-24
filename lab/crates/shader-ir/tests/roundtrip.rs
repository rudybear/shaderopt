//! M1 gate: load + re-assemble reproduces the body word for word.

mod common;

use shader_ir::lift::{roundtrip, Lifted};
use std::path::Path;

fn check(words: &[u32], what: &str) {
    let (lifted, out, rt) = roundtrip(words).unwrap();
    assert!(rt.body_identical, "{what}: body differs at word {:?}", rt.first_diff_word);
    assert_eq!(rt.words_in, rt.words_out, "{what}: length");
    // Only the generator word may differ.
    for i in [0usize, 1, 3, 4] {
        assert_eq!(rt.header_in[i], rt.header_out[i], "{what}: header word {i}");
    }
    let diff = rt.header_diff();
    assert!(diff.len() <= 1, "{what}: header diff {diff:?}");
    if let Some(d) = diff.first() {
        assert!(d.starts_with("header word 2 (generator)"), "{what}: {d}");
    }
    // Re-analysis of a re-loaded module is stable.
    let again = Lifted::load(&out).unwrap();
    assert_eq!(again.assemble(), out, "{what}: second round trip");
    assert_eq!(again.opcode_histogram(), lifted.opcode_histogram());
    if let Some(ok) = common::validate(&out) {
        assert!(ok, "{what}: spirv-val rejected the re-assembled module");
    }
}

#[test]
fn fixture_roundtrip() {
    for strip in [false, true] {
        let Some(words) = common::compile_file(Path::new(common::FIXTURE), strip) else { return };
        check(&words, &format!("tonemap.frag strip={strip}"));
    }
}

#[test]
fn corpus_roundtrip() {
    let dir = Path::new(common::CORPUS_DIR);
    if !dir.is_dir() {
        eprintln!("SKIP: no corpus directory {}", dir.display());
        return;
    }
    let mut n = 0;
    let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.extension().map_or(false, |e| e == "frag") {
            let Some(words) = common::compile_file(&p, true) else { return };
            check(&words, &p.display().to_string());
            n += 1;
        }
    }
    eprintln!("corpus: {n} shaders round-tripped");
}

#[test]
fn header_diff_is_reported() {
    let Some(words) = common::compile_file(Path::new(common::FIXTURE), true) else { return };
    let (_, _, rt) = roundtrip(&words).unwrap();
    assert_eq!(rt.header_in[2], 0x8000b, "glslang generator magic");
    assert_eq!(rt.header_diff(), vec!["header word 2 (generator): 0x8000b -> 0xf0000".to_string()]);
}
