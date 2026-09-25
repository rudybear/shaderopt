//! `shader-ir`: the shader lab's CPU model.
//!
//! * [`lift`]: a lossless lift of a SPIR-V module onto `rspirv::dr::Module` plus side tables
//!   (types, constants, def-use, block order, structured control flow, interface variables,
//!   names). `Lifted::assemble` re-emits the module; the body after the 5-word header is
//!   identical to the input.
//! * [`interp`]: an interpreter that executes the Fragment entry point per pixel in 2x2 quads,
//!   in `f64`, `f32` or `f16` numeric mode.
//! * [`npy`]: a minimal NumPy `.npy` reader/writer for float32 `(H, W, 4)` images.
//! * [`analysis`]: M2 static analysis (rates, sinks, sampler coordinate kinds, source lines).
//! * [`passes`]: M2 rewrite passes and the M3 precision demotion (`passes::demote`).

pub mod analysis;
pub mod interp;
pub mod lift;
pub mod npy;
pub mod passes;

/// Reads a `.spv` file into words (little endian).
pub fn read_spv(path: &std::path::Path) -> anyhow::Result<Vec<u32>> {
    let bytes = std::fs::read(path)
        .map_err(|e| anyhow::anyhow!("cannot read {}: {e}", path.display()))?;
    words_from_bytes(&bytes)
}

/// Converts raw SPIR-V bytes into words, honoring the magic-number endianness.
pub fn words_from_bytes(bytes: &[u8]) -> anyhow::Result<Vec<u32>> {
    if bytes.len() % 4 != 0 || bytes.len() < 20 {
        anyhow::bail!("SPIR-V binary has {} bytes; expected a multiple of 4 and >= 20", bytes.len());
    }
    let le = |c: &[u8]| u32::from_le_bytes([c[0], c[1], c[2], c[3]]);
    let be = |c: &[u8]| u32::from_be_bytes([c[0], c[1], c[2], c[3]]);
    let first = le(&bytes[..4]);
    let conv: fn(&[u8]) -> u32 = if first == spirv::MAGIC_NUMBER {
        le
    } else if be(&bytes[..4]) == spirv::MAGIC_NUMBER {
        be
    } else {
        anyhow::bail!("not a SPIR-V binary: magic {first:#x}");
    };
    Ok(bytes.chunks_exact(4).map(conv).collect())
}

/// Serializes words as little-endian bytes.
pub fn bytes_from_words(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}
