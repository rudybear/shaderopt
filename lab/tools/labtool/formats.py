"""Format quantization models, so CPU predictions can be compared with what a GPU texture can store."""
from __future__ import annotations
import numpy as np

def srgb_encode(x):
    x = np.clip(x, 0.0, 1.0)
    return np.where(x <= 0.0031308, x * 12.92, 1.055 * np.power(x, 1 / 2.4) - 0.055)

def srgb_decode(x):
    return np.where(x <= 0.04045, x / 12.92, np.power((x + 0.055) / 1.055, 2.4))

def quantize(fmt: str, img: np.ndarray) -> np.ndarray:
    out = img.astype(np.float32).copy()
    rgb, a = out[..., :3], out[..., 3:4]
    if fmt == "RGBA8":
        out = np.round(np.clip(out, 0, 1) * 255) / 255
    elif fmt == "RGBA8_SRGB":
        rgb = srgb_decode(np.round(srgb_encode(rgb) * 255) / 255)
        a = np.round(np.clip(a, 0, 1) * 255) / 255
        out = np.concatenate([rgb, a], -1)
    elif fmt in ("RGBA16F", "R16F"):
        out = out.astype(np.float16).astype(np.float32)
    elif fmt in ("RGBA32F", "R32F"):
        pass
    elif fmt == "RGB10A2":
        rgb = np.round(np.clip(rgb, 0, 1) * 1023) / 1023; a = np.round(np.clip(a, 0, 1) * 3) / 3
        out = np.concatenate([rgb, a], -1)
    elif fmt == "R11G11B10F":
        # 5-bit exponent, 6/6/5 mantissa bits, unsigned: emulate by rounding f32 mantissa
        def rf(v, mbits):
            v = np.clip(v, 0, 65024.0).astype(np.float32)
            m, e = np.frexp(v)
            return (np.round(m * (1 << mbits)) / (1 << mbits) * np.exp2(e)).astype(np.float32)
        out = np.concatenate([rf(rgb[..., 0:1], 6), rf(rgb[..., 1:2], 6), rf(rgb[..., 2:3], 5), np.ones_like(a)], -1)
    else:
        raise SystemExit(f"unknown format {fmt}")
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
