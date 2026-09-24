from __future__ import annotations
import numpy as np
from .formats import LDR_FORMATS

def _mask(a, b):
    m = np.isfinite(a).all(-1) & np.isfinite(b).all(-1)
    return m

def flip_metrics(ref: np.ndarray, test: np.ndarray, kind: str) -> dict:
    """kind: 'color' (display-encoded LDR, FLIP LDR) or 'hdr' (linear, FLIP HDR). Returns mean/p99/max of the FLIP map
    over pixels finite in both images, plus max abs difference."""
    import flip_evaluator as flip
    m = _mask(ref, test)
    r = np.nan_to_num(ref[..., :3].astype(np.float32), nan=0.0, posinf=0.0, neginf=0.0)
    t = np.nan_to_num(test[..., :3].astype(np.float32), nan=0.0, posinf=0.0, neginf=0.0)
    if kind == "color":
        r = np.clip(r, 0, 1); t = np.clip(t, 0, 1)
        res = flip.evaluate(r, t, "LDR", inputsRGB=True, applyMagma=False)
    else:
        r = np.clip(r, 0, None); t = np.clip(t, 0, None)
        res = flip.evaluate(r, t, "HDR", inputsRGB=False, applyMagma=False)
    emap = res[0] if isinstance(res, (tuple, list)) else res
    emap = np.asarray(emap, dtype=np.float32)
    if emap.ndim == 3:
        emap = emap[..., 0]
    e = emap[m] if m.any() else np.zeros(1, np.float32)
    d = np.abs(ref[..., :3] - test[..., :3])[m] if m.any() else np.zeros((1, 3), np.float32)
    return {"flip_mean": float(e.mean()), "flip_p99": float(np.quantile(e, 0.99)), "flip_max": float(e.max()),
            "abs_max": float(d.max()), "abs_p99": float(np.quantile(d, 0.99)), "masked_pixels": int((~m).sum()),
            "metric": "flip" if kind == "color" else "flip_hdr"}

def exact_metrics(ref, test) -> dict:
    m = _mask(ref, test)
    d = np.abs(ref - test)[m]
    return {"abs_max": float(d.max()) if d.size else 0.0, "mismatches": int((d > 0).any(-1).sum()) if d.size else 0,
            "masked_pixels": int((~m).sum()), "metric": "exact"}

def kind_for(pass_name: str, fmt: str, budgets: dict) -> str:
    k = budgets.get("kinds", {}).get(pass_name)
    if k:
        return k
    return "color" if fmt in LDR_FORMATS else "hdr"
