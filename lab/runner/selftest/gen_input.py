#!/usr/bin/env python
"""gen_input.py <out.npy> <W> <H> [RGBA32F|RGBA8|RGBA16F]
Writes the selftest gradient: R = x/(W-1), G = y/(H-1), B = 0.5, A = 1, float32 HxWx4, row 0 = top.
With a format, the values are quantized to it the way `lab gen-inputs` does (8-bit unorm rounding / f16 rounding), so
the file holds exactly what a texture of that format stores and a copy through such a texture must return it."""
import sys
import numpy as np

out, W, H = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
fmt = sys.argv[4] if len(sys.argv) > 4 else "RGBA32F"
x = np.arange(W, dtype=np.float32) / (W - 1)
y = np.arange(H, dtype=np.float32) / (H - 1)
img = np.empty((H, W, 4), dtype=np.float32)
img[..., 0] = x[None, :]
img[..., 1] = y[:, None]
img[..., 2] = 0.5
img[..., 3] = 1.0
if fmt == "RGBA8":
    img = (np.round(np.clip(img, 0, 1) * 255) / 255).astype(np.float32)
elif fmt == "RGBA16F":
    img = img.astype(np.float16).astype(np.float32)
elif fmt != "RGBA32F":
    sys.exit(f"unsupported selftest input format {fmt}")
np.save(out, img)
print(f"wrote {out} shape={img.shape} dtype={img.dtype} quantized_to={fmt}")
