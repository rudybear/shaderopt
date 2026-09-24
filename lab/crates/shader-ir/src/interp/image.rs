//! Image sampling: the lab's documented approximation of a Vulkan sampler with
//! `VK_FILTER_LINEAR` / `VK_FILTER_NEAREST`, no mipmaps (lod 0) and clamp-to-edge addressing.
//!
//! Formulas (Vulkan 1.3 "Texel Filtering", 2D, normalized coordinates `(s, t)`, image `W x H`):
//!
//! * unnormalized: `u = s * W`, `v = t * H` (texel `i` covers `[i, i+1)`, its center is at
//!   `i + 0.5`, i.e. `(i + 0.5) / W` in normalized space).
//! * nearest: `i = floor(u)`, `j = floor(v)`, clamped to `[0, W-1] x [0, H-1]`.
//! * linear: `i0 = floor(u - 0.5)`, `j0 = floor(v - 0.5)`, `i1 = i0 + 1`, `j1 = j0 + 1`,
//!   weights `a = (u - 0.5) - i0`, `b = (v - 0.5) - j0`; every texel index is clamped to the
//!   image after the (optional const) offset is added; result
//!   `(1-a)(1-b) T[i0,j0] + a(1-b) T[i1,j0] + (1-a) b T[i0,j1] + a b T[i1,j1]`.
//!
//! GPUs evaluate the weights in fixed point (Vulkan guarantees at least
//! `subTexelPrecisionBits` = 8 fractional bits; NVIDIA reports 8) and the blend in unspecified
//! precision. This sampler computes the weights and the blend in f64 and rounds the result to the
//! numeric mode; `weight_bits > 0` first rounds the unnormalized coordinates to fixed point with that many
//! fractional bits (round to nearest), which is what NVIDIA 580.178.04 was measured to do at 8 bits.
//! Other vendors may truncate or use more bits; verify per device.

use crate::npy::Image;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    Linear,
    Nearest,
}

impl std::str::FromStr for Filter {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> anyhow::Result<Self> {
        match s {
            "linear" => Ok(Filter::Linear),
            "nearest" => Ok(Filter::Nearest),
            other => anyhow::bail!("unknown filter {other:?}; want linear or nearest"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct BoundImage {
    pub name: String,
    pub image: Image,
    pub filter: Filter,
}

#[inline]
fn clamp_index(i: i64, n: usize) -> usize {
    i.clamp(0, n as i64 - 1) as usize
}

#[inline]
fn texel_f64(img: &Image, x: usize, y: usize) -> [f64; 4] {
    let t = img.texel(x, y);
    [t[0] as f64, t[1] as f64, t[2] as f64, t[3] as f64]
}

/// Samples at normalized coordinates `(s, t)` with an integer texel offset, lod 0.
pub fn sample_2d(img: &Image, filter: Filter, s: f64, t: f64, offset: [i64; 2], weight_bits: u32) -> [f64; 4] {
    let (w, h) = (img.width, img.height);
    if w == 0 || h == 0 {
        return [0.0; 4];
    }
    // NaN coordinates: Vulkan says undefined; pick texel 0 deterministically.
    let s = if s.is_nan() { 0.0 } else { s };
    let t = if t.is_nan() { 0.0 } else { t };
    let u = s * w as f64;
    let v = t * h as f64;
    match filter {
        Filter::Nearest => {
            let i = clamp_index(u.floor().clamp(-1e12, 1e12) as i64 + offset[0], w);
            let j = clamp_index(v.floor().clamp(-1e12, 1e12) as i64 + offset[1], h);
            texel_f64(img, i, j)
        }
        Filter::Linear => {
            let uf = (u - 0.5).clamp(-1e12, 1e12);
            let vf = (v - 0.5).clamp(-1e12, 1e12);
            // GPU model: the unnormalized coordinate is converted to fixed point with `weight_bits`
            // fractional bits by rounding to nearest BEFORE the floor, so a coordinate a hair below a
            // texel center snaps to it (weight exactly 0 on the neighbour). Measured to match NVIDIA
            // 580.178.04 at 8 bits (see lab/results/lift_check_f32.json); truncation did not.
            let (uf, vf) = if weight_bits > 0 {
                let scale = (1u64 << weight_bits.min(52)) as f64;
                ((uf * scale).round() / scale, (vf * scale).round() / scale)
            } else {
                (uf, vf)
            };
            let i0f = uf.floor();
            let j0f = vf.floor();
            let a = uf - i0f;
            let b = vf - j0f;
            let i0 = i0f as i64 + offset[0];
            let j0 = j0f as i64 + offset[1];
            let (x0, x1) = (clamp_index(i0, w), clamp_index(i0 + 1, w));
            let (y0, y1) = (clamp_index(j0, h), clamp_index(j0 + 1, h));
            let t00 = texel_f64(img, x0, y0);
            let t10 = texel_f64(img, x1, y0);
            let t01 = texel_f64(img, x0, y1);
            let t11 = texel_f64(img, x1, y1);
            let w00 = (1.0 - a) * (1.0 - b);
            let w10 = a * (1.0 - b);
            let w01 = (1.0 - a) * b;
            let w11 = a * b;
            let mut out = [0.0; 4];
            for c in 0..4 {
                out[c] = w00 * t00[c] + w10 * t10[c] + w01 * t01[c] + w11 * t11[c];
            }
            out
        }
    }
}

/// `texelFetch`: exact texel read; out-of-bounds returns zeros (what robust image access gives;
/// Vulkan without robustness leaves it undefined).
pub fn fetch_2d(img: &Image, x: i64, y: i64) -> [f64; 4] {
    if x < 0 || y < 0 || x >= img.width as i64 || y >= img.height as i64 {
        return [0.0; 4];
    }
    texel_f64(img, x as usize, y as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img2x2() -> Image {
        let mut img = Image::new(2, 2);
        img.set_texel(0, 0, [1.0, 0.0, 0.0, 1.0]);
        img.set_texel(1, 0, [0.0, 1.0, 0.0, 1.0]);
        img.set_texel(0, 1, [0.0, 0.0, 1.0, 1.0]);
        img.set_texel(1, 1, [1.0, 1.0, 1.0, 1.0]);
        img
    }

    #[test]
    fn centers_and_midpoints() {
        let img = img2x2();
        // Texel centers reproduce the texel exactly.
        assert_eq!(sample_2d(&img, Filter::Linear, 0.25, 0.25, [0, 0], 0), [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(sample_2d(&img, Filter::Linear, 0.75, 0.75, [0, 0], 0), [1.0, 1.0, 1.0, 1.0]);
        // Image center = average of all four.
        assert_eq!(sample_2d(&img, Filter::Linear, 0.5, 0.5, [0, 0], 0), [0.5, 0.5, 0.5, 1.0]);
        // Clamp to edge: beyond the border returns the edge texel.
        assert_eq!(sample_2d(&img, Filter::Linear, -3.0, 0.25, [0, 0], 0), [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(sample_2d(&img, Filter::Nearest, 0.49, 0.99, [0, 0], 0), [0.0, 0.0, 1.0, 1.0]);
        // Weight quantization: a = 0.3 truncated to 2 bits -> 0.25.
        let q = sample_2d(&img, Filter::Linear, (0.5 + 0.3) / 2.0, 0.25, [0, 0], 2);
        assert!((q[0] - 0.75).abs() < 1e-12 && (q[1] - 0.25).abs() < 1e-12, "{q:?}");
    }
}
