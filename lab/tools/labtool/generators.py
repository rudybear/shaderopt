"""Synthetic input generators. All deterministic in (seed, size). Output float32 HxWx4, row 0 = top."""
from __future__ import annotations
import numpy as np

def _grid(w, h):
    y, x = np.mgrid[0:h, 0:w].astype(np.float32)
    return (x + 0.5) / w, (y + 0.5) / h

def gradient(w, h, seed, params):
    u, v = _grid(w, h)
    img = np.stack([u, v, 0.5 * (u + v), np.ones_like(u)], -1)
    return img.astype(np.float32)

def hdr_gradient(w, h, seed, params):
    peak = float(params.get("peak", 16.0))
    u, v = _grid(w, h)
    # exponential ramp across x so both the dark toe and the HDR shoulder are present; hue varies with y
    r = peak ** u * 0.02
    img = np.stack([r * (0.6 + 0.4 * v), r * (1.0 - 0.5 * v), r * (0.3 + 0.7 * (1 - v)), np.ones_like(u)], -1)
    return img.astype(np.float32)

def edges(w, h, seed, params):
    rng = np.random.default_rng(seed)
    u, v = _grid(w, h)
    img = np.zeros((h, w, 4), np.float32); img[..., 3] = 1.0
    # thin lines at several angles, a few axis-aligned bars, and a disc
    for k in range(12):
        a = rng.uniform(0, np.pi); c = rng.uniform(-1, 1)
        d = np.abs((u * 2 - 1) * np.cos(a) + (v * 2 - 1) * np.sin(a) - c)
        img[..., k % 3] = np.maximum(img[..., k % 3], (d < 1.5 / w).astype(np.float32))
    img[:, (u[0] > 0.7) & (u[0] < 0.72)] = [1, 1, 1, 1]
    disc = ((u - 0.3) ** 2 + ((v - 0.5) * h / w) ** 2) < 0.02
    img[disc] = [0.9, 0.2, 0.1, 1]
    return img

def hdr_edges(w, h, seed, params):
    img = edges(w, h, seed, params)
    img[..., :3] *= float(params.get("peak", 32.0))
    img[..., :3] += 0.05
    return img.astype(np.float32)

def checker(w, h, seed, params):
    cells = int(params.get("cells", 8))
    u, v = _grid(w, h)
    c = ((np.floor(u * cells) + np.floor(v * cells * h / w)) % 2).astype(np.float32)
    img = np.stack([c, 1 - c, 0.5 + 0.5 * c, np.ones_like(c)], -1)
    return img.astype(np.float32)

def noise(w, h, seed, params):
    rng = np.random.default_rng(seed)
    scale = float(params.get("scale", 1.0))
    img = rng.random((h, w, 4), dtype=np.float32) * scale
    img[..., 3] = 1.0
    return img

def nan_inf(w, h, seed, params):
    """Mostly a gradient, with blocks of NaN, +Inf, -Inf, negatives and denormals."""
    img = hdr_gradient(w, h, seed, {"peak": 8.0})
    bw, bh = w // 8, h // 8
    img[0:bh, 0:bw, :3] = np.nan
    img[0:bh, bw:2 * bw, :3] = np.inf
    img[0:bh, 2 * bw:3 * bw, :3] = -np.inf
    img[bh:2 * bh, 0:bw, :3] = -0.5
    img[bh:2 * bh, bw:2 * bw, :3] = np.float32(1e-40)
    img[bh:2 * bh, 2 * bw:3 * bw, :3] = np.float32(65504.0 * 2)  # overflows f16
    return img.astype(np.float32)

GENERATORS = {f.__name__: f for f in [gradient, hdr_gradient, edges, hdr_edges, checker, noise, nan_inf]}

def generate(name: str, w: int, h: int, seed: int, params: dict) -> np.ndarray:
    if name not in GENERATORS:
        raise SystemExit(f"unknown generator {name}; have {sorted(GENERATORS)}")
    img = GENERATORS[name](w, h, seed, params or {})
    assert img.shape == (h, w, 4) and img.dtype == np.float32
    return img

# ---- G-buffer set for deferred_lit: one generator producing four named images via `gbuffer:<which>` ----
def _gbuffer(w, h, seed):
    rng = np.random.default_rng(seed)
    u, v = _grid(w, h)
    # a few spheres on a ground plane, in normalized screen space
    albedo = np.zeros((h, w, 4), np.float32); normal = np.zeros((h, w, 4), np.float32)
    shadowuv = np.zeros((h, w, 4), np.float32)
    ground = v > 0.6
    albedo[ground] = [0.5, 0.45, 0.4, 1.0]; normal[ground] = [0.5, 1.0, 0.5, 1.0]
    for k in range(6):
        cx, cy, r = rng.uniform(0.1, 0.9), rng.uniform(0.2, 0.7), rng.uniform(0.05, 0.15)
        dx, dy = (u - cx) * w / h, (v - cy)
        m = dx * dx + dy * dy < r * r
        nz = np.sqrt(np.clip(r * r - dx * dx - dy * dy, 0, None)) / r
        nrm = np.stack([dx / r, -dy / r, nz], -1) * 0.5 + 0.5
        col = rng.uniform(0.2, 0.9, 3)
        albedo[m] = [*col, 1.0]; normal[m, :3] = nrm[m]; normal[m, 3] = 1.0
    shadowuv[..., 0] = u; shadowuv[..., 1] = v; shadowuv[..., 2] = 0.5 + 0.3 * v; shadowuv[..., 3] = 1.0
    shadow = np.zeros((h, w, 4), np.float32)
    # occluder depth 0.3 in checker cells (receiver depth shadowuv.z is 0.5..0.8, so those cells ARE shadowed); 1.0 elsewhere.
    # The first version used 0.55+0.3v, which never shadowed anything and made every PCF variant look lossless.
    shadow[..., 0] = np.where(((np.floor(u * 6) + np.floor(v * 4)) % 2) == 0, 1.0, 0.3)
    shadow[..., 3] = 1.0
    return {"albedo": albedo, "normal": normal, "shadowuv": shadowuv, "shadow": shadow}

def gbuffer(w, h, seed, params):
    return _gbuffer(w, h, seed)[params.get("which", "albedo")]

GENERATORS["gbuffer"] = gbuffer
