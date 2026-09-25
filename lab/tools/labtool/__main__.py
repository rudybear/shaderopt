from __future__ import annotations
import argparse, json, sys, time, tomllib
from pathlib import Path
import numpy as np
from . import paths
from .paths import BUDGETS, RESULTS_JSONL, SHADER_IR, SPV_DIR, run, require, sha256_file, tool_versions
from .scenarios import load_scenario, all_scenarios, Scenario
from .build import build_all
from .jobs import gen_inputs, make_job, run_job, load_result_images
from .stats import median_ci, speedup_ci
from .metrics import flip_metrics, exact_metrics, kind_for
from .formats import quantize, code_error

def budgets() -> dict:
    return tomllib.loads(BUDGETS.read_text()) if BUDGETS.exists() else {}

def device_fingerprint(res: dict) -> str:
    import hashlib
    return hashlib.sha256(json.dumps(res.get("device", {}), sort_keys=True).encode()).hexdigest()[:16]

def append_result(rec: dict) -> None:
    with open(RESULTS_JSONL, "a") as f:
        f.write(json.dumps(rec) + "\n")

def summarize_timings(res: dict) -> tuple[dict, dict]:
    med, ci = {}, {}
    for p, samples in res.get("timings_ns", {}).items():
        m, c = median_ci(samples); med[p] = m; ci[p] = list(c)
    return med, ci

def cmd_build(a):
    m = build_all()
    for k, v in m["shaders"].items():
        print(f"{k:20s} {v['spv_sha256'][:12]}")
    print(f"{len(m['shaders'])} shaders built with {m['tools']['glslang']}")

def cmd_gen_inputs(a):
    for sc in (all_scenarios(a.split) if not a.scenario else [load_scenario(a.scenario)]):
        for name, p in gen_inputs(sc, force=a.force).items():
            print(f"{sc.name:24s} {name:10s} {p}")

def _run_one(sc: Scenario, variant_id: str, a, tag=None, per_pass=None):
    jd = make_job(sc, variant_id, per_pass_variant=per_pass, samples=a.samples, iterations=a.iterations, warmup=a.warmup, tag=tag)
    res, out = run_job(jd, sc, variant_id, device=a.device)
    return res, out

def gates_1_2(res: dict) -> dict:
    v = res.get("validation", {})
    return {"1": bool(res.get("ok")), "2": bool(res.get("ok")) and int(v.get("errors", 0)) == 0}

def cmd_run(a):
    sc = load_scenario(a.scenario)
    res, out = _run_one(sc, a.variant, a)
    if not res.get("ok"):
        print(f"FAILED: {res.get('error')}"); print(f"see {out}"); sys.exit(1)
    med, ci = summarize_timings(res)
    print(f"{sc.name} variant={a.variant} device={res['device'].get('name')} validation errors={res['validation'].get('errors')}")
    for p in sc.passes:
        print(f"  {p.name:12s} median {med[p.name]/1e3:9.2f} us   95% CI [{ci[p.name][0]/1e3:9.2f}, {ci[p.name][1]/1e3:9.2f}]")
    rec = {"ts": time.strftime("%Y-%m-%dT%H:%M:%S"), "scenario": sc.name, "split": sc.split, "variant_id": a.variant,
           "edit_ops": [], "device_fingerprint": device_fingerprint(res), "median_ns": med, "ci95_ns": ci, "metrics": {},
           "tolerance": {"source": "none", "value": None}, "gates": {**gates_1_2(res), "3": None, "4": None, "5": None},
           "result_dir": str(out), "tools": res["_tools"], "state": res.get("state")}
    if a.record:
        append_result(rec)
    print(f"result: {out}")

def cmd_baseline(a):
    scs = all_scenarios(a.split) if not a.scenario else [load_scenario(a.scenario)]
    for sc in scs:
        res, out = _run_one(sc, "baseline", a)
        if not res.get("ok"):
            print(f"{sc.name}: FAILED {res.get('error')} ({out})"); continue
        med, ci = summarize_timings(res)
        line = " ".join(f"{p.name}={med[p.name]/1e3:.1f}us" for p in sc.passes)
        print(f"{sc.name:24s} val_err={res['validation'].get('errors')} {line}")
        append_result({"ts": time.strftime("%Y-%m-%dT%H:%M:%S"), "scenario": sc.name, "split": sc.split, "variant_id": "baseline",
                       "edit_ops": [], "device_fingerprint": device_fingerprint(res), "median_ns": med, "ci95_ns": ci, "metrics": {},
                       "tolerance": {"source": "none", "value": None}, "gates": {**gates_1_2(res), "3": True, "4": None, "5": None},
                       "result_dir": str(out), "tools": res["_tools"], "state": res.get("state")})

def cmd_aa(a):
    """A/A noise floor: run the baseline job twice per round, alternating, and report the relative difference of medians."""
    sc = load_scenario(a.scenario)
    runs = {"A": [], "B": []}
    for r in range(a.rounds):
        for tagk in ("A", "B"):
            res, out = _run_one(sc, "baseline", a, tag=f"aa_{tagk}")
            if not res.get("ok"):
                print(f"FAILED {res.get('error')}"); sys.exit(1)
            runs[tagk].append(res)
    floor = {}
    for p in sc.passes:
        A = np.concatenate([np.asarray(r["timings_ns"][p.name]) for r in runs["A"]])
        B = np.concatenate([np.asarray(r["timings_ns"][p.name]) for r in runs["B"]])
        d, ci = speedup_ci(A, B)
        mA, cA = median_ci(A); mB, cB = median_ci(B)
        floor[p.name] = {"median_a_ns": mA, "median_b_ns": mB, "rel_diff": d, "rel_diff_ci95": list(ci),
                         "cv_a": float(np.std(A) / np.mean(A)), "n": int(A.size)}
        print(f"{sc.name} {p.name:12s} A {mA/1e3:9.2f} us  B {mB/1e3:9.2f} us  rel diff {d*100:+.2f}%  CI [{ci[0]*100:+.2f}%, {ci[1]*100:+.2f}%]  CV {floor[p.name]['cv_a']*100:.2f}%")
    fp = paths.RESULTS_DIR / sc.name / "aa_noise_floor.json"
    fp.parent.mkdir(parents=True, exist_ok=True)
    fp.write_text(json.dumps({"scenario": sc.name, "rounds": a.rounds, "samples": a.samples, "iterations": a.iterations,
                              "device": runs["A"][0].get("device"), "state": runs["A"][0].get("state"), "floor": floor}, indent=2))
    print(f"noise floor written: {fp}")

def _eval_cpu(spv: Path, w: int, h: int, mode: str, samplers: dict[str, Path], uniforms: dict, out: Path, nearest: bool = False, weight_bits: int = 8) -> None:
    require(SHADER_IR, "shader-ir binary (cargo build --release in lab/crates/shader-ir)")
    cmd = [SHADER_IR, "eval", "--spv", spv, "--width", str(w), "--height", str(h), "--mode", mode, "--out", out, "--sampler-weight-bits", str(weight_bits)]
    for n, p in samplers.items():
        cmd += ["--sampler", f"{n}={p}" + (":nearest" if nearest else "")]
    for n, v in uniforms.items():
        cmd += ["--uniform", f"{n}={json.dumps(v) if isinstance(v, list) else v}"]
    r = run(cmd)
    if r.returncode != 0:
        raise SystemExit(f"shader-ir eval failed for {spv}:\n{r.stdout}{r.stderr}")

def cmd_lift_check(a):
    """M1 gate 3: (a) empty-edit round trip is body-identical; (b) CPU f32 output matches the GPU output per pass,
    measured in code units of the storage format, feeding each pass the GPU's own readback of its inputs."""
    require(SHADER_IR, "shader-ir binary")
    tols = tomllib.loads((paths.LAB / "lift_tolerances.toml").read_text())
    scs = all_scenarios(a.split) if not a.scenario else [load_scenario(a.scenario)]
    report = []
    for sc in scs:
        res, out = _run_one(sc, "baseline", a)
        if not res.get("ok"):
            print(f"{sc.name}: runner FAILED {res.get('error')}"); continue
        gpu = load_result_images(res)
        inputs = {k: v for k, v in gen_inputs(sc).items()}
        for p in sc.passes:
            spv = SPV_DIR / f"{p.shader}.spv"
            rt = run([SHADER_IR, "roundtrip", spv]); rt_ok = rt.returncode == 0
            w, h = sc.pass_size(p)
            samplers = {sname: (inputs[src] if src in inputs else Path(res["_dir"]) / res["images"][src]) for sname, src in p.samplers.items()}
            cpu_out = Path(res["_dir"]) / f"{p.name}.cpu_{a.mode}.npy"
            _eval_cpu(spv, w, h, a.mode, samplers, p.uniforms, cpu_out, nearest=(p.sampler == "nearest"), weight_bits=a.sampler_weight_bits)
            cpu = np.load(cpu_out); g = gpu[p.name]
            m = np.isfinite(cpu).all(-1) & np.isfinite(g).all(-1)
            err = code_error(p.format, cpu, g)[m]
            t = {**tols.get("default", {}), **tols.get("shaders", {}).get(p.shader, {})}
            p99 = float(np.quantile(err, 0.99)) if err.size else 0.0; emax = float(err.max()) if err.size else 0.0
            frac1 = float((err > 1).mean()) if err.size else 0.0
            ok = rt_ok and p99 <= t["p99_codes"] and emax <= t["max_codes"]
            report.append({"scenario": sc.name, "pass": p.name, "shader": p.shader, "format": p.format, "size": [w, h], "mode": a.mode,
                           "roundtrip_ok": rt_ok, "roundtrip": rt.stdout.strip(), "p99_codes": p99, "max_codes": emax,
                           "frac_over_1_code": frac1, "masked": int((~m).sum()), "tolerance": t, "ok": ok})
            print(f"{sc.name:22s} {p.name:10s} {p.format:10s} roundtrip={'ok' if rt_ok else 'DIFF'} err p99={p99:5.1f} max={emax:6.1f} codes, >1: {frac1*100:5.2f}%  masked={int((~m).sum()):7d} {'fragile' if t.get('fragile') else ''} -> {'OK' if ok else 'FAIL'}")
    fp = paths.RESULTS_DIR / f"lift_check_{a.mode}.json"; fp.parent.mkdir(parents=True, exist_ok=True)
    fp.write_text(json.dumps({"tools": tool_versions(), "tolerances": tols, "report": report}, indent=2))
    bad = [r for r in report if not r["ok"]]
    print(f"{len(report) - len(bad)}/{len(report)} pass checks OK; report {fp}")
    sys.exit(1 if bad else 0)

def cmd_verify(a):
    """Smoke: build, generate inputs, run one small scenario, round-trip every shader."""
    build_all()
    sc = load_scenario(a.scenario)
    res, out = _run_one(sc, "baseline", a)
    ok = bool(res.get("ok")) and int(res.get("validation", {}).get("errors", 1)) == 0
    print(f"runner: {'ok' if ok else 'FAIL'} ({out})")
    if SHADER_IR.exists():
        bad = [p for p in sorted(SPV_DIR.glob("*.spv")) if run([SHADER_IR, "roundtrip", p]).returncode != 0]
        print(f"roundtrip: {len(list(SPV_DIR.glob('*.spv'))) - len(bad)} ok, {len(bad)} differ {[b.name for b in bad]}")
        ok = ok and not bad
    else:
        print("roundtrip: shader-ir not built, skipped")
    sys.exit(0 if ok else 1)

def cmd_classify(a):
    from .classify import classify
    shaders = [a.shader] if a.shader else sorted(p.stem for p in SPV_DIR.glob("*.spv") if not p.stem.endswith(".g"))
    for sh in shaders:
        r = classify(sh, max_scenarios=a.max_scenarios, stride=a.stride, sensitivity=not a.no_sensitivity, device=a.device)
        s = r["analysis"]["summary"]; print(f"{sh:18s} {s}  report lab/reports/{sh}.classification.md")

def cmd_canon(a):
    from .canon import canon
    shaders = [a.shader] if a.shader else sorted(p.stem for p in SPV_DIR.glob("*.spv") if not p.stem.endswith(".g"))
    for sh in shaders:
        r = canon(sh, rounds=a.rounds, samples=a.samples, device=a.device)
        for vid, e in r["variants"].items():
            print(f"{sh:18s} {vid:12s} ops={e['ops']} {e['by_pass']}")

def cmd_demote(a):
    from .demote import demote
    shaders = [a.shader] if a.shader else sorted(p.stem for p in SPV_DIR.glob("*.spv") if not p.stem.endswith(".g"))
    for sh in shaders:
        r = demote(sh, rounds=a.rounds, samples=a.samples, singles=a.singles, modes=tuple(a.modes.split(",")), device=a.device)
        for e in r["results"]:
            if "error" in e:
                print(f"{sh:18s} {e['variant_id']:22s} ERROR {e['error'][:80]}"); continue
            accepted = sum(1 for x in e["runs"] if x.get("gates") and all(x["gates"].get(k) for k in ("1","2","3","4")))
            print(f"{sh:18s} {e['variant_id']:22s} sites={e['n_sites']:3d} pred_p99={e['predicted']['flip_p99']:.4f} accepted_runs={accepted}/{len(e['runs'])}")

def cmd_report(a):
    from .report import write_shader_report
    shaders = [a.shader] if a.shader else sorted(p.stem for p in SPV_DIR.glob("*.spv") if not p.stem.endswith(".g"))
    for sh in shaders:
        print(write_shader_report(sh))

def cmd_graph(a):
    from .graphexp import run_graph_experiments
    scs = [a.scenario] if a.scenario else [s.name for s in all_scenarios(a.split) if len(s.passes) > 1]
    for sc in scs:
        for r in run_graph_experiments(sc, rounds=a.rounds, samples=a.samples, device=a.device):
            if "error" in r: print(f"{sc:20s} {r['experiment']:28s} ERROR {r['error'][:60]}"); continue
            print(f"{sc:20s} {r['experiment']:28s} speedup {r['speedup']*100:+.2f}% flip p99 {list(r['metrics'].values())[0]['flip_p99']:.4f} budget={r['within_budget']} timing={r['timing_gate']}")

def cmd_hypo(a):
    from .hypo import hypotheses, run_hypothesis
    shaders = [a.shader] if a.shader else sorted(p.stem for p in SPV_DIR.glob("*.spv") if not p.stem.endswith(".g"))
    for sh in shaders:
        for hdir in hypotheses(sh):
            if a.id and hdir.parent.name != a.id:
                continue
            r = run_hypothesis(sh, hdir.parent, rounds=a.rounds, samples=a.samples, device=a.device)
            pred = ", ".join(f"{k}: p99 {v['flip_p99']:.4f}" for k, v in r["prediction"].items())
            for x in r["runs"]:
                if "error" in x: print(f"{sh:16s} {r['variant_id']:16s} {x['scenario']:22s} ERROR {x['error'][:70]}"); continue
                t = list(x["timing"].values())[0]; g = x["gates"]
                print(f"{sh:16s} {r['variant_id']:16s} {x['scenario']:22s} speedup {t['speedup']*100:+6.2f}% CI[{t['speedup_ci95'][0]*100:+.2f},{t['speedup_ci95'][1]*100:+.2f}] flip p99 {max(m['flip_p99'] for m in x['metrics'].values() if 'flip_p99' in m):.4f} gates {g['1']}{g['2']}{g['3']}{g['4']} (pred {pred})")

def _print_runs(sh, vid, runs):
    for x in runs:
        if "error" in x: print(f"{sh:16s} {vid:14s} {x['scenario']:22s} ERROR {x['error'][:70]}"); continue
        t = list(x["timing"].values())[0]; g = x["gates"]
        print(f"{sh:16s} {vid:14s} {x['scenario']:22s} speedup {t['speedup']*100:+6.2f}% CI[{t['speedup_ci95'][0]*100:+.2f},{t['speedup_ci95'][1]*100:+.2f}] flip p99 {max(m.get('flip_p99', 0) for m in x['metrics'].values()):.4f} gates {g['1']}{g['2']}{g['3']}{g['4']}")

def cmd_hoist(a):
    from .m4 import hoist
    shaders = [a.shader] if a.shader else sorted(p.stem for p in SPV_DIR.glob("*.spv") if not p.stem.endswith(".g"))
    for sh in shaders:
        r = hoist(sh, rounds=a.rounds, samples=a.samples, device=a.device)
        print(f"{sh:16s} hoisted {len(r['hoisted'])} members: {[x['member'] + ':' + x.get('expr', '')[:40] for x in r['hoisted']][:6]}")
        _print_runs(sh, "hoist", r["runs"])

def cmd_approx(a):
    from .m4 import approx
    shaders = [a.shader] if a.shader else sorted(p.stem for p in SPV_DIR.glob("*.spv") if not p.stem.endswith(".g"))
    for sh in shaders:
        r = approx(sh, max_rel_err=a.max_rel_err, rounds=a.rounds, samples=a.samples, device=a.device)
        print(f"{sh:16s} sites {r['sites']} -> {len(r.get('ops', []))} approximations" + (f" ERROR {r['error'][:80]}" if r.get("error") else ""))
        if r.get("log"): print("   " + r["log"].replace("\n", "\n   ")[:600])
        _print_runs(sh, r["variant_id"], r["runs"])

def main(argv=None):
    ap = argparse.ArgumentParser(prog="lab")
    sub = ap.add_subparsers(dest="cmd", required=True)
    def common(p, samples=30):
        p.add_argument("--samples", type=int, default=samples); p.add_argument("--iterations", type=int, default=8)
        p.add_argument("--warmup", type=int, default=5); p.add_argument("--device", type=int, default=None)
    sub.add_parser("build").set_defaults(f=cmd_build)
    p = sub.add_parser("gen-inputs"); p.add_argument("--scenario"); p.add_argument("--split"); p.add_argument("--force", action="store_true"); p.set_defaults(f=cmd_gen_inputs)
    p = sub.add_parser("run"); p.add_argument("scenario"); p.add_argument("--variant", default="baseline"); p.add_argument("--record", action="store_true"); common(p); p.set_defaults(f=cmd_run)
    p = sub.add_parser("baseline"); p.add_argument("--scenario"); p.add_argument("--split"); common(p); p.set_defaults(f=cmd_baseline)
    p = sub.add_parser("aa"); p.add_argument("scenario"); p.add_argument("--rounds", type=int, default=3); common(p); p.set_defaults(f=cmd_aa)
    p = sub.add_parser("lift-check"); p.add_argument("--scenario"); p.add_argument("--split"); p.add_argument("--mode", default="f32"); p.add_argument("--sampler-weight-bits", type=int, default=8); common(p, samples=3); p.set_defaults(f=cmd_lift_check)
    p = sub.add_parser("classify"); p.add_argument("--shader"); p.add_argument("--max-scenarios", type=int, default=2); p.add_argument("--stride", type=int, default=4); p.add_argument("--no-sensitivity", action="store_true"); p.add_argument("--device", type=int, default=None); p.set_defaults(f=cmd_classify)
    p = sub.add_parser("canon"); p.add_argument("--shader"); p.add_argument("--rounds", type=int, default=2); p.add_argument("--samples", type=int, default=20); p.add_argument("--device", type=int, default=None); p.set_defaults(f=cmd_canon)
    p = sub.add_parser("demote"); p.add_argument("--shader"); p.add_argument("--rounds", type=int, default=2); p.add_argument("--samples", type=int, default=20); p.add_argument("--singles", type=int, default=6); p.add_argument("--modes", default="f16,relaxed"); p.add_argument("--device", type=int, default=None); p.set_defaults(f=cmd_demote)
    p = sub.add_parser("report"); p.add_argument("--shader"); p.set_defaults(f=cmd_report)
    p = sub.add_parser("graph"); p.add_argument("--scenario"); p.add_argument("--split", default="train"); p.add_argument("--rounds", type=int, default=2); p.add_argument("--samples", type=int, default=20); p.add_argument("--device", type=int, default=None); p.set_defaults(f=cmd_graph)
    p = sub.add_parser("hypo"); p.add_argument("--shader"); p.add_argument("--id"); p.add_argument("--rounds", type=int, default=2); p.add_argument("--samples", type=int, default=20); p.add_argument("--device", type=int, default=None); p.set_defaults(f=cmd_hypo)
    p = sub.add_parser("hoist"); p.add_argument("--shader"); p.add_argument("--rounds", type=int, default=2); p.add_argument("--samples", type=int, default=20); p.add_argument("--device", type=int, default=None); p.set_defaults(f=cmd_hoist)
    p = sub.add_parser("approx"); p.add_argument("--shader"); p.add_argument("--max-rel-err", type=float, default=1e-3); p.add_argument("--rounds", type=int, default=2); p.add_argument("--samples", type=int, default=20); p.add_argument("--device", type=int, default=None); p.set_defaults(f=cmd_approx)
    p = sub.add_parser("verify"); p.add_argument("--scenario", default="vignette_gradient"); common(p, samples=3); p.set_defaults(f=cmd_verify)
    a = ap.parse_args(argv)
    a.f(a)

if __name__ == "__main__":
    main()
