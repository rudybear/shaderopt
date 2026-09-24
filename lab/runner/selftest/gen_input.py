#!/usr/bin/env python
"""Writes the selftest gradient: R = x/(W-1), G = y/(H-1), B = 0.5, A = 1, float32 HxWx4, row 0 = top."""
import sys
import numpy as np

out, W, H = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
x = np.arange(W, dtype=np.float32) / (W - 1)
y = np.arange(H, dtype=np.float32) / (H - 1)
img = np.empty((H, W, 4), dtype=np.float32)
img[..., 0] = x[None, :]
img[..., 1] = y[:, None]
img[..., 2] = 0.5
img[..., 3] = 1.0
np.save(out, img)
print(f"wrote {out} shape={img.shape} dtype={img.dtype}")
