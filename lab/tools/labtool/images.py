from __future__ import annotations
import numpy as np
from pathlib import Path

def to_png(img: np.ndarray, path: Path, hdr: bool = False) -> None:
    import imageio.v3 as iio
    x = np.nan_to_num(img[..., :3].astype(np.float32), nan=1.0, posinf=1.0, neginf=0.0)
    if hdr:
        x = x / (1.0 + x); x = np.power(np.clip(x, 0, 1), 1 / 2.2)
    iio.imwrite(path, (np.clip(x, 0, 1) * 255 + 0.5).astype(np.uint8))

def to_exr(img: np.ndarray, path: Path) -> None:
    import OpenEXR
    h, w = img.shape[:2]
    ch = {"R": img[..., 0].astype(np.float32), "G": img[..., 1].astype(np.float32), "B": img[..., 2].astype(np.float32), "A": img[..., 3].astype(np.float32)}
    f = OpenEXR.File(ch)  # OpenEXR >= 3.3 python API
    f.write(str(path))
