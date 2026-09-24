//! Minimal NumPy `.npy` support for lab images: `float32`, C order, shape `(H, W, 4)`.
//!
//! The reader also accepts `(H, W)`, `(H, W, 1..=4)` and `<f8` data, widening or padding to RGBA
//! (missing G/B = 0, missing A = 1). The writer always emits `<f4` `(H, W, 4)` in C order.

use anyhow::{anyhow, bail, Context, Result};
use std::path::Path;

/// An RGBA float32 image, row 0 at the top, `data.len() == height * width * 4`.
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub data: Vec<f32>,
}

impl Image {
    pub fn new(width: usize, height: usize) -> Self {
        Image { width, height, data: vec![0.0; width * height * 4] }
    }

    pub fn filled(width: usize, height: usize, value: f32) -> Self {
        Image { width, height, data: vec![value; width * height * 4] }
    }

    #[inline]
    pub fn texel(&self, x: usize, y: usize) -> [f32; 4] {
        let i = (y * self.width + x) * 4;
        [self.data[i], self.data[i + 1], self.data[i + 2], self.data[i + 3]]
    }

    #[inline]
    pub fn set_texel(&mut self, x: usize, y: usize, v: [f32; 4]) {
        let i = (y * self.width + x) * 4;
        self.data[i..i + 4].copy_from_slice(&v);
    }
}

pub fn read(path: &Path) -> Result<Image> {
    let bytes = std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    parse(&bytes).with_context(|| format!("invalid .npy {}", path.display()))
}

pub fn write(path: &Path, img: &Image) -> Result<()> {
    let bytes = serialize(img);
    std::fs::write(path, bytes).with_context(|| format!("cannot write {}", path.display()))
}

pub fn serialize(img: &Image) -> Vec<u8> {
    assert_eq!(img.data.len(), img.width * img.height * 4);
    let dict = format!(
        "{{'descr': '<f4', 'fortran_order': False, 'shape': ({}, {}, 4), }}",
        img.height, img.width
    );
    // magic(6) + version(2) + header_len(2) + dict + padding + '\n' must be a multiple of 64.
    let base = 6 + 2 + 2;
    let mut header = dict.into_bytes();
    let pad = (64 - (base + header.len() + 1) % 64) % 64;
    header.extend(std::iter::repeat(b' ').take(pad));
    header.push(b'\n');
    let mut out = Vec::with_capacity(base + header.len() + img.data.len() * 4);
    out.extend_from_slice(b"\x93NUMPY\x01\x00");
    out.extend_from_slice(&(header.len() as u16).to_le_bytes());
    out.extend_from_slice(&header);
    for v in &img.data {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

pub fn parse(bytes: &[u8]) -> Result<Image> {
    if bytes.len() < 10 || &bytes[..6] != b"\x93NUMPY" {
        bail!("missing NUMPY magic");
    }
    let (major, minor) = (bytes[6], bytes[7]);
    let (header_len, data_start) = match major {
        1 => (u16::from_le_bytes([bytes[8], bytes[9]]) as usize, 10),
        2 | 3 => {
            if bytes.len() < 12 {
                bail!("truncated header");
            }
            (u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize, 12)
        }
        _ => bail!("unsupported .npy version {major}.{minor}"),
    };
    let header = bytes
        .get(data_start..data_start + header_len)
        .ok_or_else(|| anyhow!("truncated header"))?;
    let header = std::str::from_utf8(header).context("header is not UTF-8")?;
    let descr = dict_value(header, "descr").ok_or_else(|| anyhow!("no descr in header"))?;
    let descr = descr.trim_matches(|c| c == '\'' || c == '"');
    let fortran = dict_value(header, "fortran_order").unwrap_or("False");
    if fortran.trim() != "False" {
        bail!("fortran_order must be False");
    }
    let shape_str = dict_value(header, "shape").ok_or_else(|| anyhow!("no shape in header"))?;
    let shape: Vec<usize> = shape_str
        .trim()
        .trim_start_matches('(')
        .trim_end_matches(')')
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.parse::<usize>().map_err(|e| anyhow!("bad shape {shape_str}: {e}")))
        .collect::<Result<_>>()?;
    let (height, width, channels) = match shape.as_slice() {
        [h, w] => (*h, *w, 1),
        [h, w, c] if (1..=4).contains(c) => (*h, *w, *c),
        _ => bail!("unsupported shape {:?}; want (H, W, 4)", shape),
    };
    let payload = &bytes[data_start + header_len..];
    let count = height * width * channels;
    let scalars: Vec<f32> = match descr {
        "<f4" => {
            if payload.len() < count * 4 {
                bail!("payload too short: {} bytes for {} float32", payload.len(), count);
            }
            payload[..count * 4]
                .chunks_exact(4)
                .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect()
        }
        "<f8" => {
            if payload.len() < count * 8 {
                bail!("payload too short: {} bytes for {} float64", payload.len(), count);
            }
            payload[..count * 8]
                .chunks_exact(8)
                .map(|c| f64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]) as f32)
                .collect()
        }
        other => bail!("unsupported dtype {other}; want '<f4'"),
    };
    let mut img = Image::new(width, height);
    for p in 0..height * width {
        let mut t = [0.0f32, 0.0, 0.0, 1.0];
        for c in 0..channels {
            t[c] = scalars[p * channels + c];
        }
        img.data[p * 4..p * 4 + 4].copy_from_slice(&t);
    }
    Ok(img)
}

/// Finds `'key': value` in the header dict; returns the raw value text up to the next top-level
/// comma or the closing brace.
fn dict_value<'a>(header: &'a str, key: &str) -> Option<&'a str> {
    let pat_sq = format!("'{key}'");
    let pat_dq = format!("\"{key}\"");
    let start = header.find(&pat_sq).map(|i| i + pat_sq.len())
        .or_else(|| header.find(&pat_dq).map(|i| i + pat_dq.len()))?;
    let rest = &header[start..];
    let rest = rest.trim_start().strip_prefix(':')?.trim_start();
    let mut depth = 0i32;
    for (i, ch) in rest.char_indices() {
        match ch {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            ',' | '}' if depth == 0 => return Some(rest[..i].trim()),
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let mut img = Image::new(3, 2);
        for (i, v) in img.data.iter_mut().enumerate() {
            *v = i as f32 * 0.25;
        }
        let bytes = serialize(&img);
        assert_eq!((bytes.len() - img.data.len() * 4) % 64, 0);
        let back = parse(&bytes).unwrap();
        assert_eq!(back, img);
    }
}
