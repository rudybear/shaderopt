"""Format quantization models, so CPU predictions can be compared with what a GPU texture can store.

`quantize` is also what `gen_inputs` applies to an input image before saving it in its declared format
(CONTRACTS.md "Scenario"): the runner's encoder (runner/src/formats.cpp encodeFromFloatRGBA) is exact on
already-quantized values, so the CPU model reads from the .npy exactly what the GPU samples."""
from __future__ import annotations
import numpy as np

FORMATS = ("RGBA8", "RGBA8_SRGB", "RGBA16F", "RGBA32F", "R11G11B10F", "RGB10A2", "R16F", "R32F")

def srgb_encode(x):
    x = np.clip(x, 0.0, 1.0)
    return np.where(x <= 0.0031308, x * 12.92, 1.055 * np.power(x, 1 / 2.4) - 0.055)

def srgb_decode(x):
    return np.where(x <= 0.04045, x / 12.92, np.power((x + 0.055) / 1.055, 2.4))

def quantize(fmt: str, img: np.ndarray) -> np.ndarray:
    """Model what the GPU store does to the value. For UNORM/sRGB 8-bit and 10-bit targets the conversion of NaN
    yields 0 and +/-Inf clamps (what this device does and what the runner reads back), so the CPU prediction must
    not keep NaN there, or the comparison masks exactly the pixels that go black on the device."""
    out = img.astype(np.float32).copy()
    if fmt in ("RGBA8", "RGBA8_SRGB", "RGB10A2"):
        out = np.nan_to_num(out, nan=0.0, posinf=1.0, neginf=0.0)
    rgb, a = out[..., :3], out[..., 3:4]
    if fmt == "RGBA8":
        out = np.round(np.clip(out, 0, 1) * 255) / 255
    elif fmt == "RGBA8_SRGB":
        rgb = srgb_decode(np.round(srgb_encode(rgb) * 255) / 255)
        a = np.round(np.clip(a, 0, 1) * 255) / 255
        out = np.concatenate([rgb, a], -1)
    elif fmt in ("RGBA16F", "R16F"):
        with np.errstate(over="ignore"):  # |x| > 65504 becomes Inf, as on the device
            out = out.astype(np.float16).astype(np.float32)
    elif fmt in ("RGBA32F", "R32F"):
        pass
    elif fmt == "RGB10A2":
        rgb = np.round(np.clip(rgb, 0, 1) * 1023) / 1023; a = np.round(np.clip(a, 0, 1) * 3) / 3
        out = np.concatenate([rgb, a], -1)
    elif fmt == "R11G11B10F":
        # Unsigned floats with a 5-bit exponent (bias 15) and 6/6/5 fraction bits; mirrors the runner's packUFloat:
        # NaN and negatives -> 0, round to nearest even, above the largest finite value -> Inf (like f16), and the
        # fixed subnormal spacing 2^(-14-mbits) below 2^-14. frexp gives m in [0.5, 1), whose leading bit is the
        # implicit 1, so mbits fraction bits are mbits+1 significant bits of m.
        def rf(v, mbits):
            v = np.maximum(np.nan_to_num(v, nan=0.0, posinf=np.inf, neginf=0.0), 0).astype(np.float32)
            with np.errstate(invalid="ignore", over="ignore"):
                m, e = np.frexp(v)
                scale = 1 << (mbits + 1)
                normal = np.round(m * scale) / scale * np.exp2(e)
                step = 2.0 ** (-14 - mbits)
                sub = np.round(v / step) * step
                out = np.where(v < 2.0 ** -14, sub, normal)
            return np.where(out >= 65536.0, np.inf, out).astype(np.float32)
        out = np.concatenate([rf(rgb[..., 0:1], 6), rf(rgb[..., 1:2], 6), rf(rgb[..., 2:3], 5), np.ones_like(a)], -1)
    else:
        raise SystemExit(f"unknown format {fmt}")
    if fmt in ("R16F", "R32F"):
        # single-channel storage: sampling (and readback) returns (r, 0, 0, 1)
        out = np.concatenate([out[..., 0:1], np.zeros_like(out[..., 1:3]), np.ones_like(out[..., 3:4])], -1)
    return out.astype(np.float32)

LDR_FORMATS = {"RGBA8", "RGBA8_SRGB", "RGB10A2"}


def code_error(fmt: str, a: np.ndarray, b: np.ndarray) -> np.ndarray:
    """Per-pixel max-over-RGB error in code units of the storage format (8-bit codes, or ULPs of f16/f32)."""
    a = a[..., :3].astype(np.float32); b = b[..., :3].astype(np.float32)
    if fmt in ("RGBA8", "RGB10A2"):
        n = 255 if fmt == "RGBA8" else 1023
        return np.abs(np.round(np.clip(a, 0, 1) * n) - np.round(np.clip(b, 0, 1) * n)).max(-1)
    if fmt == "RGBA8_SRGB":
        return np.abs(np.round(srgb_encode(a) * 255) - np.round(srgb_encode(b) * 255)).max(-1)
    if fmt in ("RGBA16F", "R16F"):
        ia = a.astype(np.float16).view(np.int16).astype(np.int64); ib = b.astype(np.float16).view(np.int16).astype(np.int64)
        # map sign-magnitude to a monotonic integer line so ULP distance across zero is right
        ia = np.where(ia < 0, -(ia & 0x7FFF), ia); ib = np.where(ib < 0, -(ib & 0x7FFF), ib)
        return np.abs(ia - ib).max(-1)
    if fmt in ("RGBA32F", "R32F", "R11G11B10F"):
        ia = a.view(np.int32).astype(np.int64); ib = b.view(np.int32).astype(np.int64)
        ia = np.where(ia < 0, -(ia & 0x7FFFFFFF), ia); ib = np.where(ib < 0, -(ib & 0x7FFFFFFF), ib)
        return np.abs(ia - ib).max(-1)
    raise SystemExit(f"unknown format {fmt}")
