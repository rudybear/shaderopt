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


def steady_clock_mask(res: dict, tol: float = 0.05) -> tuple[list[bool] | None, dict]:
    """Mobile DVFS: keep the samples taken at the top clock seen in this run (within tol of the max). Returns (mask, info);
    mask is None when the runner did not record per-sample clocks or fewer than 10 samples qualify."""
    clocks = res.get("sample_clock_mhz")
    if not clocks:
        return None, {"steady": False, "reason": "no per-sample clocks"}
    import numpy as np
    c = np.asarray(clocks, dtype=np.float64); top = float(np.nanmax(c)) if c.size else 0.0
    mask = (c >= top * (1.0 - tol)).tolist()
    n = int(sum(mask))
    if n < 10:
        return None, {"steady": False, "reason": f"only {n} samples at the top clock {top:.0f} MHz", "top_clock_mhz": top, "n_top": n}
    return mask, {"steady": True, "top_clock_mhz": top, "n_top": n, "n_total": int(c.size), "min_clock_mhz": float(np.nanmin(c))}

def timings_steady(res: dict, pass_name: str):
    """Timings of one pass restricted to steady-clock samples (or all samples when no clock data)."""
    import numpy as np
    t = np.asarray(res["timings_ns"][pass_name], dtype=np.float64)
    mask, info = steady_clock_mask(res)
    return (t[np.asarray(mask)] if mask is not None else t), info
