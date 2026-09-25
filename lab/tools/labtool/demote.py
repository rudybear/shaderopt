"""M3 precision demotion: single sites and greedy cumulative sets, relaxed and explicit f16, measured per device."""
from __future__ import annotations
import json
from pathlib import Path
from .paths import LAB, SHADER_IR, SPV_DIR, VARIANTS, run, require
from .experiment import measure_variant, evaluate_gates, record, write_variant, scenarios_for_shader, budgets
from .metrics import kind_for
from .report import write_shader_report

ANALYSIS = LAB / "analysis"

def tolerance_for(shader: str, pass_name: str, fmt: str, bud: dict) -> tuple[dict | None, str]:
    """Effective tolerance via the precedence chain (site/shader annotations not yet supported): budgets.toml -> pipeline default."""
    if not bud.get("pipeline", {}).get("allow_lossy", False):
        return None, "none"
    kind = kind_for(pass_name, fmt, bud)
    t = dict(bud.get("defaults", {}).get(kind, {}))
    t.update(bud.get("targets", {}).get(pass_name, {}))
    t["min_speedup"] = bud["pipeline"].get("min_speedup", 0.0)
    return t, "budgets"

def demote_variant(shader: str, sites: list[int], mode: str, vid: str) -> tuple[Path, list]:
    d = VARIANTS / shader / vid; d.mkdir(parents=True, exist_ok=True)
    out = d / f"{shader}.spv"; ops = d / "ops.json"
    cmd = [SHADER_IR, "demote", "--spv", SPV_DIR / f"{shader}.spv", "--out", out, "--sites", ",".join(map(str, sites)), "--mode", mode, "--ops", ops]
    if mode == "f16":
        cmd.append("--group-converts")
    r = run(cmd)
    if r.returncode != 0:
        raise RuntimeError(f"demote failed ({shader} {mode} {len(sites)} sites): {r.stdout}{r.stderr}".strip())
    return out, json.loads(ops.read_text())

def demote(shader: str, rounds: int = 2, samples: int = 20, singles: int = 6, modes=("f16", "relaxed"), device=None) -> dict:
    require(SHADER_IR, "shader-ir")
    an = json.loads((ANALYSIS / f"{shader}.json").read_text())
    ins = {str(i["id"]): i for i in an["analysis"]["instructions"]}
    sens = {k: v for k, v in an["sensitivity"].items() if k != "all"}
    if not sens:
        raise SystemExit(f"no sensitivity data for {shader}; run lab classify first")
    ranked = sorted(sens.items(), key=lambda kv: (kv[1]["flip_p99"], kv[1]["flip_mean"]))
    bud = budgets()
    ctx = an["sensitivity_context"]; tol, tol_src = tolerance_for(shader, ctx["pass"], ctx["format"], bud)
    p99_budget = (tol or {}).get("p99_max", 0.05)
    sets = {}
    zero = [int(k) for k, v in ranked if v["flip_p99"] == 0.0 and v["flip_mean"] == 0.0]
    if zero:
        sets["zero"] = zero
    for frac, name in ((0.25, "q4"), (0.5, "q2"), (1.0, "q1")):
        s = [int(k) for k, v in ranked if v["flip_p99"] <= p99_budget * frac]
        if s and s not in sets.values():
            sets[name] = s
    allc = [int(k) for k, _ in ranked]
    if allc not in sets.values():
        sets["all"] = allc
    for k, _ in ranked[:singles]:
        sets[f"s{k}"] = [int(k)]
    results = []
    scs_train = scenarios_for_shader(shader, "train"); scs_hold = scenarios_for_shader(shader, "holdout")
    for mode in modes:
        for name, sites in sets.items():
            if mode == "relaxed" and name.startswith("s"):
                continue  # single-site RelaxedPrecision is not worth GPU time on desktop
            vid = f"demote_{mode}_{name}"
            try:
                spv, ops = demote_variant(shader, sites, mode, vid)
            except RuntimeError as e:
                results.append({"variant_id": vid, "mode": mode, "set": name, "sites": sites, "error": str(e)[:400]}); continue
            pred = {"flip_p99": max(sens[str(s)]["flip_p99"] for s in sites), "flip_mean_max_site": max(sens[str(s)]["flip_mean"] for s in sites)}
            write_variant(shader, vid, spv, ops, extra={"mode": mode, "set": name, "sites": sites, "predicted": pred})
            entry = {"variant_id": vid, "mode": mode, "set": name, "sites": sites, "n_sites": len(sites), "predicted": pred, "ops": len(ops), "runs": []}
            for sc in scs_train + scs_hold:
                m = measure_variant(shader, vid, sc, rounds=rounds if sc.split == "train" else 1, samples=samples, device=device)
                if not m.get("ok"):
                    entry["runs"].append({"scenario": sc.name, "error": m.get("error")}); continue
                g_strict = evaluate_gates(m, shader, sc, None); g_bud = evaluate_gates(m, shader, sc, tol)
                rec = record(m, g_bud, ops, tol_src, tol, extra={"mode": mode, "set": name, "sites": sites, "gates_strict": g_strict, "predicted": pred})
                entry["runs"].append({"scenario": sc.name, "split": sc.split, "gates": g_bud, "gates_strict": g_strict,
                                      "timing": {k: v for k, v in m["timing"].items() if v["touched"]}, "metrics": m["metrics"]})
            results.append(entry)
    out = {"shader": shader, "tolerance": tol, "tolerance_source": tol_src, "sets": sets, "results": results}
    (ANALYSIS / f"{shader}.demote.json").write_text(json.dumps(out, indent=1))
    write_shader_report(shader)
    return out
