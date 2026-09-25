"""M4 graph-level experiments that need no shader change: intermediate formats and resolution scaling of passes.
Each experiment is a derived scenario measured against the original scenario's baseline (whole-chain time and final quality)."""
from __future__ import annotations
import copy, json, time
from pathlib import Path
import numpy as np
from .paths import LAB, RESULTS_JSONL, RESULTS_DIR
from .scenarios import Scenario, load_scenario, all_scenarios
from .jobs import make_job, run_job, load_result_images, drop_images
from .stats import median_ci, speedup_ci
from .metrics import flip_metrics, kind_for
from .experiment import budgets, noise_floor

REPORTS = LAB / "reports"
FORMAT_ALTS = {"RGBA16F": ["R11G11B10F", "RGBA8"], "RGBA32F": ["RGBA16F", "R11G11B10F"]}

def _derive(sc: Scenario, pass_name: str, fmt: str | None = None, scale: float | None = None) -> Scenario:
    d = copy.deepcopy(sc)
    for p in d.passes:
        if p.name == pass_name:
            if fmt: p.format = fmt
            if scale is not None: p.scale = scale
    return d

def _chain_ns(res_list, passes):
    return np.concatenate([np.sum([np.asarray(r["timings_ns"][p.name]) for p in passes], axis=0) for r in res_list])

def run_graph_experiments(scenario: str, rounds: int = 2, samples: int = 20, device=None) -> list[dict]:
    sc = load_scenario(scenario)
    bud = budgets()
    experiments = []
    n = len(sc.passes)
    for idx, p in enumerate(sc.passes):
        is_final = idx == n - 1
        if not is_final:
            for alt in FORMAT_ALTS.get(p.format, []):
                experiments.append((f"fmt_{p.name}_{alt}", _derive(sc, p.name, fmt=alt), {"kind": "format", "pass": p.name, "from": p.format, "to": alt}))
            for s in (0.5, 0.25):
                if p.scale > s:
                    experiments.append((f"res_{p.name}_{s}", _derive(sc, p.name, scale=s), {"kind": "resolution", "pass": p.name, "from": p.scale, "to": s}))
    out = []
    for name, dsc, meta in experiments:
        base, var = [], []
        for r in range(rounds):
            jb = make_job(sc, "baseline", samples=samples, tag=f"gx_base_{name}"); rb, _ = run_job(jb, sc, "baseline", device=device)
            jv = make_job(dsc, "baseline", samples=samples, tag=f"gx_var_{name}"); rv, _ = run_job(jv, dsc, f"graph_{name}", device=device)
            if not rb.get("ok") or not rv.get("ok"):
                out.append({"experiment": name, **meta, "error": rb.get("error") or rv.get("error")}); break
            base.append(rb); var.append(rv)
        else:
            B = _chain_ns(base, sc.passes); V = _chain_ns(var, sc.passes)
            sp, ci = speedup_ci(B, V)
            bi = load_result_images(base[-1]); vi = load_result_images(var[-1])
            metrics = {}
            for q in sc.quality_outputs:
                fmt = next(x.format for x in sc.passes if x.name == q)
                metrics[q] = flip_metrics(bi[q], vi[q], kind_for(q, fmt, bud))
            floor = noise_floor(sc) or 0.0
            tol = bud.get("defaults", {}).get(kind_for(sc.quality_outputs[0], next(x.format for x in sc.passes if x.name == sc.quality_outputs[0]), bud), {})
            within = all(m["flip_p99"] <= tol.get("p99_max", 1e9) and m["flip_mean"] <= tol.get("mean_max", 1e9) for m in metrics.values())
            need = max(0.02, floor, bud.get("pipeline", {}).get("min_speedup", 0.0))
            rec = {"ts": time.strftime("%Y-%m-%dT%H:%M:%S"), "experiment": name, **meta, "scenario": sc.name, "split": sc.split,
                   "chain_baseline_ns": float(np.median(B)), "chain_variant_ns": float(np.median(V)), "speedup": sp, "speedup_ci95": list(ci),
                   "metrics": metrics, "within_budget": within, "timing_gate": bool(sp >= need and ci[0] > 0), "required": need,
                   "validation_errors": var[-1]["validation"].get("errors"), "device": var[-1]["device"].get("name"), "result_dir": var[-1]["_dir"]}
            drop_images(*base, *var)
            out.append(rec)
            with open(RESULTS_JSONL, "a") as f:
                f.write(json.dumps({"variant_id": f"graph_{name}", "shader": None, "graph": True, **rec}) + "\n")
    (LAB / "analysis" / f"{sc.name}.graph.json").write_text(json.dumps(out, indent=1))
    write_graph_report(sc.name, out)
    return out

def write_graph_report(scenario: str, out: list[dict]):
    L = [f"# {scenario}: graph-level experiments (formats, resolution)", "", "Whole-chain GPU time vs the unmodified chain, interleaved; quality on the chain's judged outputs. These are proposals for the app, never applied automatically.", "",
         "| experiment | kind | pass | from | to | chain baseline us | chain variant us | speedup | 95% CI | FLIP mean / p99 (final) | within budget | timing gate | verdict |", "|---|---|---|---|---|---|---|---|---|---|---|---|---|"]
    for r in out:
        if "error" in r:
            L.append(f"| {r['experiment']} | {r['kind']} | {r['pass']} | {r['from']} | {r['to']} | | | | | | | | runner error: {r['error'][:80]} |"); continue
        m = list(r["metrics"].values())[0]
        verdict = "PROPOSE to app" if r["within_budget"] and r["timing_gate"] else ("faster but over budget" if r["timing_gate"] else ("within budget, no speedup" if r["within_budget"] else "rejected"))
        L.append(f"| {r['experiment']} | {r['kind']} | {r['pass']} | {r['from']} | {r['to']} | {r['chain_baseline_ns']/1e3:.2f} | {r['chain_variant_ns']/1e3:.2f} | {r['speedup']*100:+.2f}% | [{r['speedup_ci95'][0]*100:+.2f}%, {r['speedup_ci95'][1]*100:+.2f}%] | {m['flip_mean']:.4f} / {m['flip_p99']:.4f} | {'yes' if r['within_budget'] else 'no'} | {'yes' if r['timing_gate'] else 'no'} | {verdict} |")
    (REPORTS / f"{scenario}.graph.md").write_text("\n".join(L) + "\n")
