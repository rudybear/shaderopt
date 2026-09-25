"""M2 exact canonicalization: rewrite passes -> variants -> CPU verification -> GPU A/B -> lab/reports/<shader>.canonical.md"""
from __future__ import annotations
import json
from pathlib import Path
import numpy as np
from .paths import LAB, SHADER_IR, SPV_DIR, VARIANTS, run, require
from .jobs import make_job, run_job, gen_inputs
from .experiment import measure_variant, evaluate_gates, record, write_variant, scenarios_for_shader, noise_floor
from .classify import _eval, _pass_inputs, REPORTS
from .formats import code_error

PIPELINES = {"canon_exact": "fold,dce,cse,ident,unroll,select", "canon_full": "fold,dce,cse,ident,unroll,select,divconst,powspec"}

def rewrite(shader: str, variant_id: str, passes: str) -> tuple[Path, list]:
    d = VARIANTS / shader / variant_id; d.mkdir(parents=True, exist_ok=True)
    out = d / f"{shader}.spv"; ops = d / "ops.json"
    r = run([SHADER_IR, "rewrite", "--spv", SPV_DIR / f"{shader}.spv", "--out", out, "--passes", passes, "--ops", ops])
    if r.returncode != 0:
        raise SystemExit(f"rewrite failed for {shader} ({passes}):\n{r.stdout}{r.stderr}")
    return out, json.loads(ops.read_text())

def cpu_verify(shader: str, variant_spv: Path, device=None) -> dict:
    """f64 model: original vs variant on the first train scenario, feeding GPU readbacks for chained inputs."""
    sc = scenarios_for_shader(shader, "train")[0]
    jd = make_job(sc, "baseline", samples=1, iterations=1, warmup=0, tag="canon_verify")
    res, _ = run_job(jd, sc, "baseline", device=device)
    inputs = gen_inputs(sc); out = {}
    for p in sc.passes:
        if p.shader != shader:
            continue
        w, h = sc.pass_size(p); sm = _pass_inputs(sc, p, res, inputs)
        a = VARIANTS / shader / "tmp_orig.npy"; b = VARIANTS / shader / "tmp_var.npy"
        for mode in ("f64", "f32"):
            _eval(SPV_DIR / f"{shader}.spv", w, h, sm, p.uniforms, a, [], nearest=(p.sampler == "nearest"), mode=mode)
            _eval(variant_spv, w, h, sm, p.uniforms, b, [], nearest=(p.sampler == "nearest"), mode=mode)
            A = np.load(a); B = np.load(b); m = np.isfinite(A).all(-1) & np.isfinite(B).all(-1)
            bit = bool(np.array_equal(A.view(np.uint32)[m], B.view(np.uint32)[m])) and bool((np.isfinite(A).all(-1) == np.isfinite(B).all(-1)).all())
            ulp = code_error("RGBA32F", A, B)[m]
            out[f"{p.name}:{mode}"] = {"bit_identical": bit, "max_ulp_f32": float(ulp.max()) if ulp.size else 0.0, "abs_max": float(np.abs(A - B)[m].max()) if m.any() else 0.0, "masked": int((~m).sum())}
        a.unlink(missing_ok=True); b.unlink(missing_ok=True)
    return out

def canon(shader: str, rounds: int = 2, samples: int = 20, device=None) -> dict:
    require(SHADER_IR, "shader-ir")
    report = {"shader": shader, "variants": {}}
    for vid, passes in PIPELINES.items():
        spv, ops = rewrite(shader, vid, passes)
        write_variant(shader, vid, spv, ops, extra={"passes": passes})
        classes = {c: sum(1 for o in ops if o["class"] == c) for c in ("exact", "ulp")}
        entry = {"passes": passes, "ops": len(ops), "classes": classes, "by_pass": {}, "verify": None, "measure": []}
        for o in ops:
            entry["by_pass"][o["pass"]] = entry["by_pass"].get(o["pass"], 0) + 1
        if not ops:
            report["variants"][vid] = entry; continue
        entry["verify"] = cpu_verify(shader, spv, device=device)
        for sc in scenarios_for_shader(shader, "train"):
            m = measure_variant(shader, vid, sc, rounds=rounds, samples=samples, device=device)
            if not m.get("ok"):
                entry["measure"].append({"scenario": sc.name, "error": m.get("error")}); continue
            gates = evaluate_gates(m, shader, sc, None)
            rec = record(m, gates, ops, "none", None, extra={"pipeline": passes})
            entry["measure"].append({"scenario": sc.name, "gates": gates, "timing": {k: v for k, v in m["timing"].items() if v["touched"]}, "metrics": m["metrics"], "noise_floor": noise_floor(sc)})
        report["variants"][vid] = entry
    (LAB / "analysis" / f"{shader}.canon.json").write_text(json.dumps(report, indent=1))
    write_canonical_report(report)
    return report

def write_canonical_report(r: dict) -> Path:
    L = [f"# {r['shader']}: exact canonicalization", "", "Variants produced by `shader-ir rewrite` from the baseline SPIR-V, verified in the CPU model (f64 and f32) against the original on the first train scenario, then measured on the desktop GPU interleaved with the baseline (B,V,B,V...). `exact` ops are bit-identical by IEEE semantics; `ulp` ops are identical in real arithmetic with bounded rounding differences.", ""]
    for vid, e in r["variants"].items():
        L += [f"## {vid}", "", f"Passes: `{e['passes']}`. Edit ops: {e['ops']} ({', '.join(f'{k}={v}' for k, v in e['classes'].items())}); by pass: {', '.join(f'{k}={v}' for k, v in sorted(e['by_pass'].items())) or 'none'}.", ""]
        if not e["ops"]:
            L += ["No pass fired: the baseline is already canonical for this pipeline.", ""]; continue
        L += ["CPU verification:", ""]
        for k, v in (e["verify"] or {}).items():
            L.append(f"- {k}: {'bit-identical' if v['bit_identical'] else 'DIFFERS'}; max {v['max_ulp_f32']:.0f} f32 ULP, abs max {v['abs_max']:.3g}, masked {v['masked']}")
        L += ["", "GPU measurement (desktop):", "", "| scenario | pass | baseline us | variant us | speedup | 95% CI | gates 1-4 | verdict |", "|---|---|---|---|---|---|---|---|"]
        for m in e["measure"]:
            if "error" in m:
                L.append(f"| {m['scenario']} | | | | | | | runner error: {m['error']} |"); continue
            g = m["gates"]
            for pname, t in m["timing"].items():
                sp = t["speedup"]; ci = t["speedup_ci95"]; nf = m.get("noise_floor")
                if ci[0] <= 0.0 <= ci[1] or abs(sp) < max(0.02, nf or 0):
                    verdict = "no measurable effect (driver already does this, or nothing to gain)"
                elif sp > 0:
                    verdict = "faster" + (" and ACCEPTED" if all(g[k] for k in ("1", "2", "3", "4")) else " but a gate failed")
                else:
                    verdict = "SLOWER: rewrite hurts on this driver"
                L.append(f"| {m['scenario']} | {pname} | {t['baseline_median_ns']/1e3:.2f} | {t['variant_median_ns']/1e3:.2f} | {sp*100:+.2f}% | [{ci[0]*100:+.2f}%, {ci[1]*100:+.2f}%] | {g['1']} {g['2']} {g['3']} {g['4']} | {verdict} |")
        L.append("")
    out = REPORTS / f"{r['shader']}.canonical.md"; out.write_text("\n".join(L) + "\n"); return out
