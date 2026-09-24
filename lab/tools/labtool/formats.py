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
