"""Variant measurement: interleaved baseline/variant GPU runs, quality metrics, gates. Reused by M2..M5."""
from __future__ import annotations
import json, time, tomllib
from pathlib import Path
import numpy as np
from .paths import RESULTS_DIR, RESULTS_JSONL, VARIANTS, SPV_DIR, BUDGETS, run, SPIRV_VAL, tool_versions
from .scenarios import Scenario, all_scenarios
from .jobs import make_job, run_job, load_result_images, drop_images
from .stats import median_ci, speedup_ci, timings_steady, paired_speedup
from .metrics import flip_metrics, exact_metrics, kind_for

def budgets() -> dict:
    return tomllib.loads(BUDGETS.read_text()) if BUDGETS.exists() else {}

def scenarios_for_shader(shader: str, split: str | None = None) -> list[Scenario]:
    return [sc for sc in all_scenarios(split) if any(p.shader == shader for p in sc.passes)]

def noise_floor(sc: Scenario) -> float | None:
    fp = RESULTS_DIR / sc.name / "aa_noise_floor.json"
    if not fp.exists():
        return None
    d = json.loads(fp.read_text())
    return max(abs(v["rel_diff"]) for v in d["floor"].values())

def throttled(res: dict) -> bool:
    """Android thermal gate: labtool.android marks state.throttled when ThermalStatus stayed >= SEVERE after the cooldown retries."""
    return any(isinstance(res.get(k), dict) and res[k].get("throttled") for k in ("state", "state_after"))

def measure_variant(shader: str, variant_id: str, sc: Scenario, rounds: int = 3, samples: int = 30, iterations: int = 8,
                    warmup: int = 5, device=None, android: str | None = None, inflight: int = 3) -> dict:
    """Interleave baseline and variant runs (B, V, B, V, ...) on one scenario. Returns timings, images and metrics."""
    per_pass = {p.name: (variant_id if p.shader == shader else "baseline") for p in sc.passes}
    base, var = [], []
    for r in range(rounds):
        jb = make_job(sc, "baseline", samples=samples, iterations=iterations, warmup=warmup, tag=f"ab_base_{variant_id}", inflight=inflight)
        rb, _ = run_job(jb, sc, "baseline", device=device, android=android)
        jv = make_job(sc, variant_id, per_pass_variant=per_pass, samples=samples, iterations=iterations, warmup=warmup, tag=f"ab_var_{variant_id}", inflight=inflight)
        rv, _ = run_job(jv, sc, variant_id, device=device, android=android)
        if not rb.get("ok") or not rv.get("ok"):
            return {"ok": False, "error": rb.get("error") or rv.get("error"), "baseline": rb, "variant": rv}
        if throttled(rb) or throttled(rv):
            drop_images(rb, rv); continue   # thermally throttled sample: dropped, never averaged in
        base.append(rb); var.append(rv)
    if not base:
        return {"ok": False, "error": f"all {rounds} rounds dropped: device thermally throttled (state.throttled)", "baseline": None, "variant": None}
    out = {"ok": True, "scenario": sc.name, "split": sc.split, "variant_id": variant_id, "shader": shader,
           "device": var[-1]["device"], "state": var[-1].get("state"), "validation": var[-1]["validation"],
           "validation_baseline": base[-1]["validation"], "timing": {}, "metrics": {}, "result_dir": var[-1]["_dir"],
           "baseline_dir": base[-1]["_dir"], "tools": tool_versions()}
    b_img = load_result_images(base[-1]); v_img = load_result_images(var[-1])
    bud = budgets()
    for p in sc.passes:
        Bs = [timings_steady(r, p.name) for r in base]; Vs = [timings_steady(r, p.name) for r in var]
        B = np.concatenate([b[0] for b in Bs]); V = np.concatenate([v[0] for v in Vs])
        sp, ci = speedup_ci(B, V); mb, cb = median_ci(B); mv, cv = median_ci(V)
        out["timing"][p.name] = {"baseline_median_ns": mb, "variant_median_ns": mv, "speedup": sp, "speedup_ci95": list(ci),
                                 "baseline_ci95": list(cb), "variant_ci95": list(cv), "n": int(B.size), "touched": p.shader == shader,
                                 "clock": {"baseline": [b[1] for b in Bs], "variant": [v[1] for v in Vs]},
                                 "paired": dict(zip(("speedup", "ci95", "rounds"), paired_speedup([b[0] for b in Bs], [v[0] for v in Vs])))}
    for name in sc.quality_outputs + [p.name for p in sc.passes if p.shader == shader]:
        if name in out["metrics"]:
            continue
        fmt = next(p.format for p in sc.passes if p.name == name)
        kind = kind_for(name, fmt, bud)
        out["metrics"][name] = exact_metrics(b_img[name], v_img[name]) if kind == "mask" else flip_metrics(b_img[name], v_img[name], kind)
    drop_images(*base, *var)
    return out

def evaluate_gates(m: dict, shader: str, sc: Scenario, tolerance: dict | None) -> dict:
    """Gates 1-4 per the brief. `tolerance` = None means no lossy budget: quality must be within the format's noise (exact metric 0 or FLIP p99 <= 0)."""
    g = {"1": bool(m.get("ok")), "2": bool(m.get("ok")) and int(m["validation"].get("errors", 0)) <= int(m["validation_baseline"].get("errors", 0)),
         "3": None, "4": None, "5": None}
    if not m.get("ok"):
        return g
    # gate 3: quality on every downstream output
    ok3 = True
    for name, met in m["metrics"].items():
        if met["metric"] == "exact":
            ok3 &= met["mismatches"] == 0
        elif tolerance is None:
            ok3 &= met["flip_max"] == 0.0 and met["masked_pixels"] == 0
        else:
            ok3 &= met["flip_mean"] <= tolerance.get("mean_max", 1e9) and met["flip_p99"] <= tolerance.get("p99_max", 1e9)
    g["3"] = bool(ok3)
    # gate 4: timing on the touched passes (chain total if several)
    touched = [t for t in m["timing"].values() if t["touched"]]
    if touched:
        floor = noise_floor(sc) or 0.0
        need = max(0.02, floor)
        if tolerance is not None and tolerance.get("min_speedup") is not None:
            need = max(need, float(tolerance["min_speedup"]))
        sp = sum(t["baseline_median_ns"] - t["variant_median_ns"] for t in touched) / sum(t["baseline_median_ns"] for t in touched)
        lo = min(t["speedup_ci95"][0] for t in touched)
        mobile = (m.get("state") or {}).get("source") == "android"
        if mobile and all(t.get("paired", {}).get("ci95") for t in touched):
            sp = min(t["paired"]["speedup"] for t in touched); lo = min(t["paired"]["ci95"][0] for t in touched)
            g["estimator"] = "paired-rounds"
        g["4"] = bool(sp >= need and lo > 0.0)
        g["speedup"] = sp; g["required"] = need
    return g

def record(m: dict, gates: dict, edit_ops: list, tolerance_src: str, tolerance: dict | None, extra: dict | None = None) -> dict:
    import hashlib
    rec = {"ts": time.strftime("%Y-%m-%dT%H:%M:%S"), "scenario": m["scenario"], "split": m["split"], "variant_id": m["variant_id"],
           "shader": m["shader"], "edit_ops": edit_ops, "device_fingerprint": hashlib.sha256(json.dumps(m["device"], sort_keys=True).encode()).hexdigest()[:16],
           "median_ns": {k: v["variant_median_ns"] for k, v in m["timing"].items()},
           "baseline_median_ns": {k: v["baseline_median_ns"] for k, v in m["timing"].items()},
           "speedup": {k: [v["speedup"], v["speedup_ci95"]] for k, v in m["timing"].items() if v["touched"]},
           "metrics": m["metrics"], "tolerance": {"source": tolerance_src, "value": tolerance}, "gates": gates,
           "result_dir": m["result_dir"], "tools": m["tools"], "state": m.get("state"), **(extra or {})}
    with open(RESULTS_JSONL, "a") as f:
        f.write(json.dumps(rec) + "\n")
    return rec

def write_variant(shader: str, variant_id: str, spv_src: Path, ops: list, parent: str = "baseline", extra: dict | None = None) -> Path:
    d = VARIANTS / shader / variant_id; d.mkdir(parents=True, exist_ok=True)
    dst = d / f"{shader}.spv"
    if spv_src.resolve() != dst.resolve():
        dst.write_bytes(spv_src.read_bytes())
    r = run([SPIRV_VAL, dst])
    meta = {"shader": shader, "variant_id": variant_id, "parent": parent, "edit_ops": ops, "spirv_val": r.returncode == 0,
            "spirv_val_output": (r.stdout + r.stderr).strip(), "tools": tool_versions(), "created": time.strftime("%Y-%m-%dT%H:%M:%S"), **(extra or {})}
    (d / "variant.json").write_text(json.dumps(meta, indent=2))
    return d
