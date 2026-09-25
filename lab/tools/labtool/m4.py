"""M4 shader-level transforms driven through shader-ir: uniform hoisting (exact, needs CPU-computed uniforms) and
range-profiled polynomial approximation (lossy)."""
from __future__ import annotations
import copy, json
from pathlib import Path
import numpy as np
from .paths import LAB, SHADER_IR, SPV_DIR, VARIANTS, run, require
from .scenarios import Scenario
from .jobs import make_job, run_job, gen_inputs
from .experiment import evaluate_gates, record, write_variant, scenarios_for_shader, budgets
from .demote import tolerance_for
from .hypo import measure_variant_derived
from .classify import _eval, _pass_inputs
from .formats import quantize
from .metrics import flip_metrics, kind_for
from .report import write_shader_report

ANALYSIS = LAB / "analysis"

def _predict(shader: str, variant_spv: Path, extra_u_fn, device=None) -> dict:
    sc = scenarios_for_shader(shader, "train")[0]
    jd = make_job(sc, "baseline", samples=1, iterations=1, warmup=0, tag="m4_pred"); res, _ = run_job(jd, sc, "baseline", device=device)
    inputs = gen_inputs(sc); out = {}
    for p in sc.passes:
        if p.shader != shader:
            continue
        w, h = sc.pass_size(p); sm = _pass_inputs(sc, p, res, inputs)
        a = variant_spv.parent / "pb.npy"; b = variant_spv.parent / "pv.npy"
        _eval(SPV_DIR / f"{shader}.spv", w, h, sm, p.uniforms, a, [], nearest=(p.sampler == "nearest"))
        u = {**p.uniforms, **(extra_u_fn(p.uniforms) if extra_u_fn else {})}
        _eval(variant_spv, w, h, sm, u, b, [], nearest=(p.sampler == "nearest"))
        out[p.name] = flip_metrics(quantize(p.format, np.load(a)), quantize(p.format, np.load(b)), kind_for(p.name, p.format, budgets()))
        a.unlink(); b.unlink()
    return out

def _measure_all(shader: str, vid: str, ops: list, extra_u_fn, rounds, samples, device) -> list:
    bud = budgets(); runs = []
    for sc in scenarios_for_shader(shader, "train") + scenarios_for_shader(shader, "holdout"):
        dsc = copy.deepcopy(sc)
        for p in dsc.passes:
            if p.shader == shader and extra_u_fn:
                p.uniforms = {**p.uniforms, **extra_u_fn(p.uniforms)}
        m = measure_variant_derived(shader, vid, sc, dsc, {}, rounds=rounds if sc.split == "train" else 1, samples=samples, device=device)
        if not m.get("ok"):
            runs.append({"scenario": sc.name, "error": m.get("error")}); continue
        p = next(x for x in sc.passes if x.shader == shader); tol, src = tolerance_for(shader, p.name, p.format, bud)
        g = evaluate_gates(m, shader, sc, tol); gs = evaluate_gates(m, shader, sc, None)
        record(m, g, ops, src, tol, extra={"gates_strict": gs, "m4": vid})
        runs.append({"scenario": sc.name, "split": sc.split, "gates": g, "gates_strict": gs, "timing": {k: v for k, v in m["timing"].items() if v["touched"]}, "metrics": m["metrics"]})
    return runs

# ---------------- hoisting ----------------
def hoist(shader: str, rounds: int = 2, samples: int = 20, device=None) -> dict:
    require(SHADER_IR, "shader-ir")
    vid = "hoist"; d = VARIANTS / shader / vid; d.mkdir(parents=True, exist_ok=True)
    pre = d / "pre.spv"; out = d / f"{shader}.spv"; plan = d / "plan.json"; ops1 = d / "ops_pre.json"; ops2 = d / "ops.json"
    r = run([SHADER_IR, "rewrite", "--spv", SPV_DIR / f"{shader}.spv", "--out", pre, "--passes", "fold,dce,cse,ident,unroll", "--ops", ops1])
    if r.returncode != 0:
        raise SystemExit(f"rewrite failed: {r.stdout}{r.stderr}")
    r = run([SHADER_IR, "hoist", "--spv", pre, "--out", out, "--ops", ops2, "--plan", plan, "--min-ops", "2"])
    if r.returncode != 0:
        raise SystemExit(f"hoist failed: {r.stdout}{r.stderr}")
    ops = json.loads(ops1.read_text()) + json.loads(ops2.read_text()); planv = json.loads(plan.read_text())
    res = {"shader": shader, "variant_id": vid, "hoisted": planv, "ops": len(ops), "runs": [], "prediction": {}}
    if not planv:
        (ANALYSIS / f"{shader}.hoist.json").write_text(json.dumps(res, indent=1)); return res
    write_variant(shader, vid, out, ops, extra={"plan": planv, "pipeline": "fold,dce,cse,ident,unroll + hoist"})
    ids = [str(x["source_id"]) for x in planv]
    def extra_u(uniforms: dict) -> dict:
        # evaluate the hoisted expressions with the CPU model on the pre-hoist module (uniform values are pixel-independent)
        dump = d / "dump.json"
        _eval(pre, 2, 2, {}, uniforms, d / "dump_out.npy", ["--dump-ids", ",".join(ids), "--dump", dump], mode="f32")
        vals = json.loads(dump.read_text()); (d / "dump_out.npy").unlink(missing_ok=True)
        return {x["member"]: (vals[str(x["source_id"])] if len(vals[str(x["source_id"])]) > 1 else vals[str(x["source_id"])][0]) for x in planv}
    # samplers are needed by eval even for the dump: give it the real ones
    def extra_u_real(uniforms: dict) -> dict:
        sc = scenarios_for_shader(shader, "train")[0]; p = next(x for x in sc.passes if x.shader == shader)
        inputs = gen_inputs(sc)
        # chained inputs: use zeros of the right size (values are uniform-rate, samplers do not matter)
        sm = {}
        for sname, src in p.samplers.items():
            if src in inputs:
                sm[sname] = inputs[src]
            else:
                z = d / f"zero_{sname}.npy"
                if not z.exists():
                    np.save(z, np.zeros((2, 2, 4), np.float32))
                sm[sname] = z
        dump = d / "dump.json"
        _eval(pre, 2, 2, sm, uniforms, d / "dump_out.npy", ["--dump-ids", ",".join(ids), "--dump", dump], mode="f32")
        vals = json.loads(dump.read_text()); (d / "dump_out.npy").unlink(missing_ok=True)
        return {x["member"]: (vals[str(x["source_id"])] if len(vals[str(x["source_id"])]) > 1 else vals[str(x["source_id"])][0]) for x in planv}
    res["prediction"] = _predict(shader, out, extra_u_real, device=device)
    res["runs"] = _measure_all(shader, vid, ops, extra_u_real, rounds, samples, device)
    (ANALYSIS / f"{shader}.hoist.json").write_text(json.dumps(res, indent=1)); write_shader_report(shader)
    return res

# ---------------- approximation ----------------
TRANSCENDENTALS = {"Exp", "Exp2", "Log", "Log2", "Pow", "Sin", "Cos", "Sqrt", "InverseSqrt"}

def approx(shader: str, max_rel_err: float = 1e-3, rounds: int = 2, samples: int = 20, device=None) -> dict:
    require(SHADER_IR, "shader-ir")
    an = json.loads((ANALYSIS / f"{shader}.json").read_text())["analysis"]
    sites = [i["id"] for i in an["instructions"] if i.get("ext") in TRANSCENDENTALS and not i["sinks"] and i["rate"] != "const"]
    ranges = ANALYSIS / shader / "ranges.json"
    vid = f"approx_{str(max_rel_err).replace('.', 'p')}"; d = VARIANTS / shader / vid; d.mkdir(parents=True, exist_ok=True)
    out = d / f"{shader}.spv"; ops = d / "ops.json"
    res = {"shader": shader, "variant_id": vid, "sites": sites, "max_rel_err": max_rel_err, "runs": [], "prediction": {}, "log": ""}
    if not sites:
        (ANALYSIS / f"{shader}.approx.json").write_text(json.dumps(res, indent=1)); return res
    r = run([SHADER_IR, "approx", "--spv", SPV_DIR / f"{shader}.spv", "--out", out, "--ops", ops, "--ranges", ranges, "--sites", ",".join(map(str, sites)), "--max-rel-err", str(max_rel_err)])
    res["log"] = (r.stdout + r.stderr).strip()
    if r.returncode != 0:
        res["error"] = res["log"]; (ANALYSIS / f"{shader}.approx.json").write_text(json.dumps(res, indent=1)); return res
    opsv = json.loads(ops.read_text()); res["ops"] = opsv
    if not opsv:
        (ANALYSIS / f"{shader}.approx.json").write_text(json.dumps(res, indent=1)); return res
    write_variant(shader, vid, out, opsv, extra={"sites": sites, "max_rel_err": max_rel_err, "log": res["log"]})
    res["prediction"] = _predict(shader, out, None, device=device)
    res["runs"] = _measure_all(shader, vid, opsv, None, rounds, samples, device)
    (ANALYSIS / f"{shader}.approx.json").write_text(json.dumps(res, indent=1)); write_shader_report(shader)
    return res
