"""Semantic rewrite hypotheses: compile, validate, predict in the CPU model, measure on the device, gate, record."""
from __future__ import annotations
import copy, importlib.util, json, tomllib
from pathlib import Path
import numpy as np
from .paths import LAB, GLSLANG, SPIRV_VAL, SPV_DIR, VARIANTS, run
from .build import compile_glsl
from .scenarios import Scenario
from .jobs import make_job, run_job, gen_inputs, load_result_images
from .experiment import measure_variant, evaluate_gates, record, write_variant, scenarios_for_shader, budgets
from .demote import tolerance_for
from .classify import _eval, _pass_inputs
from .formats import quantize
from .metrics import flip_metrics, kind_for
from .report import write_shader_report

HYP = LAB / "hypotheses"

def _load_helper(d: Path, fname: str, func: str):
    f = d / fname
    if not f.exists():
        return None
    spec = importlib.util.spec_from_file_location(f"hyp_{d.parent.name}_{d.name}_{fname[:-3]}", f); mod = importlib.util.module_from_spec(spec); spec.loader.exec_module(mod)
    return getattr(mod, func)

def _derived_scenario(sc: Scenario, shader: str, extra_u, extra_in) -> tuple[Scenario, dict]:
    d = copy.deepcopy(sc); extra_arrays = {}
    for p in d.passes:
        if p.shader != shader:
            continue
        if extra_u:
            p.uniforms = {**p.uniforms, **extra_u(p.uniforms)}
        if extra_in:
            arrs = extra_in(p.uniforms)
            for name, arr in arrs.items():
                iname = f"{p.name}_{name}"; extra_arrays[iname] = arr
                p.samplers = {**p.samplers, name: iname}
                d.inputs.append({"name": iname, "file": f"__extra__/{iname}.npy"})
    return d, extra_arrays

def hypotheses(shader: str) -> list[Path]:
    return sorted(p for p in (HYP / shader).glob("*/hypothesis.toml")) if (HYP / shader).exists() else []

def run_hypothesis(shader: str, hdir: Path, rounds: int = 2, samples: int = 20, device=None) -> dict:
    meta = tomllib.loads((hdir / "hypothesis.toml").read_text())["hypothesis"]
    hid = meta["id"]; vid = f"hyp_{hid}"
    vdir = VARIANTS / shader / vid; vdir.mkdir(parents=True, exist_ok=True)
    spv = vdir / f"{shader}.spv"
    compile_glsl(hdir / f"{shader}.frag", spv)
    (vdir / f"{shader}.frag").write_text((hdir / f"{shader}.frag").read_text())
    ops = [{"pass": "hypothesis", "class": meta["class"], "target": 0, "replaced_by": 0, "detail": meta["claim"]}]
    write_variant(shader, vid, spv, ops, extra={"hypothesis": meta})
    extra_u = _load_helper(hdir, "uniforms.py", "extra_uniforms"); extra_in = _load_helper(hdir, "inputs.py", "extra_inputs")
    bud = budgets(); out = {"hypothesis": meta, "variant_id": vid, "prediction": {}, "runs": []}
    scs = scenarios_for_shader(shader, "train") + scenarios_for_shader(shader, "holdout")
    # ---- CPU prediction on the first train scenario: variant model vs baseline model (both f32, quantized to the pass format)
    sc0 = scs[0]; dsc, extra_arrays = _derived_scenario(sc0, shader, extra_u, extra_in)
    jd = make_job(sc0, "baseline", samples=1, iterations=1, warmup=0, tag=f"hyp_pred_{hid}")
    res, _ = run_job(jd, sc0, "baseline", device=device)
    inputs = gen_inputs(sc0)
    for p0, p1 in zip(sc0.passes, dsc.passes):
        if p0.shader != shader:
            continue
        w, h = sc0.pass_size(p0)
        sm0 = _pass_inputs(sc0, p0, res, inputs)
        sm1 = dict(sm0)
        for sname, iname in p1.samplers.items():
            if iname in extra_arrays:
                pth = vdir / f"{iname}.npy"; np.save(pth, extra_arrays[iname]); sm1[sname] = pth
        a = vdir / "pred_base.npy"; b = vdir / "pred_var.npy"
        _eval(SPV_DIR / f"{shader}.spv", w, h, sm0, p0.uniforms, a, [], nearest=(p0.sampler == "nearest"))
        _eval(spv, w, h, sm1, p1.uniforms, b, [], nearest=(p1.sampler == "nearest"))
        kind = kind_for(p0.name, p0.format, bud)
        out["prediction"][p0.name] = flip_metrics(quantize(p0.format, np.load(a)), quantize(p0.format, np.load(b)), kind)
        a.unlink(); b.unlink()
    # ---- device measurement on every scenario using the shader
    for sc in scs:
        dsc, extra_arrays = _derived_scenario(sc, shader, extra_u, extra_in)
        m = measure_variant_derived(shader, vid, sc, dsc, extra_arrays, rounds=rounds if sc.split == "train" else 1, samples=samples, device=device)
        if not m.get("ok"):
            out["runs"].append({"scenario": sc.name, "error": m.get("error")}); continue
        p = next(x for x in sc.passes if x.shader == shader)
        tol, src = tolerance_for(shader, p.name, p.format, bud)
        g_strict = evaluate_gates(m, shader, sc, None); g = evaluate_gates(m, shader, sc, tol)
        record(m, g, ops, src, tol, extra={"hypothesis": hid, "gates_strict": g_strict, "predicted": out["prediction"]})
        out["runs"].append({"scenario": sc.name, "split": sc.split, "gates": g, "gates_strict": g_strict, "timing": {k: v for k, v in m["timing"].items() if v["touched"]}, "metrics": m["metrics"]})
    (LAB / "analysis" / f"{shader}.hyp_{hid}.json").write_text(json.dumps(out, indent=1))
    write_shader_report(shader)
    return out

def measure_variant_derived(shader, variant_id, sc, dsc, extra_arrays, rounds, samples, device):
    """Like experiment.measure_variant, but the variant runs on a derived scenario (extra uniforms/inputs) while the baseline runs on the original."""
    from .experiment import tool_versions
    from .stats import median_ci, speedup_ci
    from .metrics import exact_metrics
    per_pass = {p.name: (variant_id if p.shader == shader else "baseline") for p in sc.passes}
    base, var = [], []
    for r in range(rounds):
        jb = make_job(sc, "baseline", samples=samples, tag=f"ab_base_{variant_id}"); rb, _ = run_job(jb, sc, "baseline", device=device)
        jv = make_job(dsc, variant_id, per_pass_variant=per_pass, samples=samples, tag=f"ab_var_{variant_id}", extra_inputs={k: v for k, v in extra_arrays.items()})
        # extra inputs referenced as __extra__ files: rewrite the job inputs to the copies make_job wrote
        rv, _ = run_job(jv, dsc, variant_id, device=device)
        if not rb.get("ok") or not rv.get("ok"):
            return {"ok": False, "error": rb.get("error") or rv.get("error")}
        base.append(rb); var.append(rv)
    out = {"ok": True, "scenario": sc.name, "split": sc.split, "variant_id": variant_id, "shader": shader, "device": var[-1]["device"], "state": var[-1].get("state"),
           "validation": var[-1]["validation"], "validation_baseline": base[-1]["validation"], "timing": {}, "metrics": {}, "result_dir": var[-1]["_dir"], "baseline_dir": base[-1]["_dir"], "tools": tool_versions()}
    b_img = load_result_images(base[-1]); v_img = load_result_images(var[-1]); bud = budgets()
    for p in sc.passes:
        B = np.concatenate([np.asarray(r["timings_ns"][p.name]) for r in base]); V = np.concatenate([np.asarray(r["timings_ns"][p.name]) for r in var])
        sp, ci = speedup_ci(B, V); mb, cb = median_ci(B); mv, cv = median_ci(V)
        out["timing"][p.name] = {"baseline_median_ns": mb, "variant_median_ns": mv, "speedup": sp, "speedup_ci95": list(ci), "baseline_ci95": list(cb), "variant_ci95": list(cv), "n": int(B.size), "touched": p.shader == shader}
    for name in dict.fromkeys(sc.quality_outputs + [p.name for p in sc.passes if p.shader == shader]):
        fmt = next(p.format for p in sc.passes if p.name == name); kind = kind_for(name, fmt, bud)
        out["metrics"][name] = exact_metrics(b_img[name], v_img[name]) if kind == "mask" else flip_metrics(b_img[name], v_img[name], kind)
    return out
