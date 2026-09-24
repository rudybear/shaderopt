from __future__ import annotations
import numpy as np

def median_ci(samples, n_boot: int = 2000, seed: int = 0, alpha: float = 0.05):
    x = np.asarray(samples, dtype=np.float64)
    x = x[np.isfinite(x)]
    if x.size == 0:
        return float("nan"), (float("nan"), float("nan"))
    rng = np.random.default_rng(seed)
    idx = rng.integers(0, x.size, size=(n_boot, x.size))
    meds = np.median(x[idx], axis=1)
    return float(np.median(x)), (float(np.quantile(meds, alpha / 2)), float(np.quantile(meds, 1 - alpha / 2)))

def speedup_ci(base, var, n_boot: int = 2000, seed: int = 0):
    """Relative improvement (base_median - var_median) / base_median with a bootstrap CI."""
    b = np.asarray(base, dtype=np.float64); v = np.asarray(var, dtype=np.float64)
    b = b[np.isfinite(b)]; v = v[np.isfinite(v)]
    rng = np.random.default_rng(seed)
    bi = rng.integers(0, b.size, size=(n_boot, b.size)); vi = rng.integers(0, v.size, size=(n_boot, v.size))
    r = (np.median(b[bi], axis=1) - np.median(v[vi], axis=1)) / np.median(b[bi], axis=1)
    point = (np.median(b) - np.median(v)) / np.median(b)
    return float(point), (float(np.quantile(r, 0.025)), float(np.quantile(r, 0.975)))
