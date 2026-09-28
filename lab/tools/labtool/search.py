"""M5 search: compose typed edit-op genes per shader, predict every genome in the CPU model (the genome space is small
enough to enumerate), then spend a bounded GPU budget measuring the predicted frontier and its single-gene neighbours.
Pareto selection is on (measured speedup, measured error) per device; predictions only decide what to measure."""
from __future__ import annotations
import copy, itertools, json, random, re, shutil, time
from pathlib import Path
import numpy as np
from .jobs import TARGET as _TARGET

def _mobile() -> bool:
    """On mobile every scenario, holdout included, gets the full round count: paired rounds are the only estimator that survives DVFS drift."""
    return bool(_TARGET.get("android"))

from .paths import LAB, SHADER_IR, SPV_DIR, VARIANTS, run, require
from .build import compile_glsl
from .jobs import make_job, run_job, gen_inputs
from .experiment import evaluate_gates, record, write_variant, scenarios_for_shader, budgets
from .demote import tolerance_for
from .hypo import measure_variant_derived, HYP, _load_helper
from .classify import _eval, _pass_inputs
from .formats import quantize
from .metrics import flip_metrics, kind_for
from .report import write_shader_report

SEARCH = LAB / "analysis" / "search"
APPROX_LEVELS = [None, 1e-2, 1e-3, 1e-4]
DEMOTE_LEVELS = ["none", "zero", "q4", "q2", "q1", "all"]
EXACT_PASSES = "fold,dce,cse,ident,unroll,select"

def _sources(shader: str) -> list[tuple[str, Path, dict]]:
    """Source variants: the baseline plus every hypothesis GLSL that needs no extra inputs (extra uniforms are fine)."""
    out = [("baseline", SPV_DIR / f"{shader}.spv", {})]
    for hdir in sorted(p.parent for p in (HYP / shader).glob("*/hypothesis.toml")) if (HYP / shader).exists() else []:
        if (hdir / "inputs.py").exists():
            continue
        spv = SEARCH / shader / f"src_{hdir.name}.spv"; spv.parent.mkdir(parents=True, exist_ok=True)
        compile_glsl(hdir / f"{shader}.frag", spv)
        out.append((hdir.name, spv, {"extra_u": _load_helper(hdir, "uniforms.py", "extra_uniforms")}))
    return out

class Context:
    """Per-shader, per-source cached analysis: candidate sites, per-site sensitivity, ranges, transcendental sites."""
    def __init__(self, shader: str, source: str, spv: Path, extra_u, device=None):
        self.shader, self.source, self.spv, self.extra_u, self.device = shader, source, spv, extra_u, device
        self.dir = SEARCH / shader / source; self.dir.mkdir(parents=True, exist_ok=True)
        self.scenes = []   # one entry per scenario using the shader: (scenario, pass, samplers, reference image)
        for sc in scenarios_for_shader(shader, "train") + scenarios_for_shader(shader, "holdout"):
            p = next(x for x in sc.passes if x.shader == shader); w, h = sc.pass_size(p)
            jd = make_job(sc, "baseline", samples=1, iterations=1, warmup=0, tag="search_ctx"); res, _ = run_job(jd, sc, "baseline", device=device)
            if not res.get("ok"):
                continue
            inputs = gen_inputs(sc); sm = _pass_inputs(sc, p, res, inputs); ref = self.dir / f"ref_{sc.name}.npy"
            _eval(SPV_DIR / f"{shader}.spv", w, h, sm, p.uniforms, ref, [], nearest=(p.sampler == "nearest"))   # reference = ORIGINAL shader
            idx = sc.passes.index(p)
            downstream = [(q, _pass_inputs(sc, q, res, inputs), sc.pass_size(q)) for q in sc.passes[idx + 1:]]
            judged = [q for q in sc.quality_outputs if q != p.name and q in [d[0].name for d in downstream]]
            scene = {"sc": sc, "p": p, "w": w, "h": h, "samplers": sm, "res": res, "inputs": inputs, "downstream": downstream, "judged": judged,
                     "ref_q": quantize(p.format, np.load(ref)), "kind": kind_for(p.name, p.format, budgets()), "ref_chain": {}}
            if judged:
                scene["ref_chain"] = self._chain(scene, ref, "ref")
            self.scenes.append(scene)
        s0 = self.scenes[0]
        self.sc, self.p, self.w, self.h, self.samplers = s0["sc"], s0["p"], s0["w"], s0["h"], s0["samplers"]
        self.uniforms = {**self.p.uniforms, **(extra_u(self.p.uniforms) if extra_u else {})}
        self.kind = s0["kind"]; self.ref_q = s0["ref_q"]
        cache = self.dir / "context.json"
        if cache.exists():
            c = json.loads(cache.read_text()); self.sens = c["sens"]; self.trans = c["trans"]
        else:
            self.sens, self.trans = self._analyze(); cache.write_text(json.dumps({"sens": self.sens, "trans": self.trans}))

    def _chain(self, scene: dict, pass_out: Path, tag: str) -> dict[str, np.ndarray]:
        """Run the downstream (baseline) passes in the CPU model, feeding `pass_out` as this pass's output. Returns the judged outputs."""
        outs = {scene["p"].name: pass_out}; result = {}
        for q, sm, (qw, qh) in scene["downstream"]:
            sm2 = {k: (outs[scene["sc"].passes[[x.name for x in scene["sc"].passes].index(k2)].name] if False else v) for k, v in sm.items()}
            # replace samplers that read an output we recomputed
            for sname, src in q.samplers.items():
                if src in outs: sm2[sname] = outs[src]
            o = self.dir / f"chain_{tag}_{scene['sc'].name}_{q.name}.npy"
            _eval(SPV_DIR / f"{q.shader}.spv", qw, qh, sm2, q.uniforms, o, [], nearest=(q.sampler == "nearest"))
            outs[q.name] = o
            if q.name in scene["judged"]:
                result[q.name] = quantize(q.format, np.load(o))
        return result

    def _eval(self, spv: Path, out: Path, extra: list | None = None, uniforms: dict | None = None):
        _eval(spv, self.w, self.h, self.samplers, uniforms if uniforms is not None else self.uniforms, out, extra or [], nearest=(self.p.sampler == "nearest"))

    def predict(self, spv: Path, uniforms: dict | None = None, extra_members: dict | None = None) -> dict:
        """Worst FLIP over every scenario (train and holdout) in the CPU model. `extra_members` are per-scenario extra uniforms
        (hoisted values, hypothesis helpers) keyed by scenario name; falls back to `uniforms` for the first scene."""
        worst = None
        for i, s in enumerate(self.scenes):
            u = dict(s["p"].uniforms)
            if self.extra_u: u.update(self.extra_u(s["p"].uniforms))
            if extra_members and s["sc"].name in extra_members: u.update(extra_members[s["sc"].name])
            elif i == 0 and uniforms is not None: u = uniforms
            tmp = self.dir / f"pred_{int(time.time()*1e6)}.npy"
            _eval(spv, s["w"], s["h"], s["samplers"], u, tmp, [], nearest=(s["p"].sampler == "nearest"))
            m = flip_metrics(s["ref_q"], quantize(s["p"].format, np.load(tmp)), s["kind"])
            if s["judged"]:   # every downstream judged output, through the baseline passes in the CPU model
                var_chain = self._chain(s, tmp, "var")
                for q, ref_q in s["ref_chain"].items():
                    qp = next(x for x in s["sc"].passes if x.name == q)
                    mq = flip_metrics(ref_q, var_chain[q], kind_for(q, qp.format, budgets()))
                    if (mq["flip_p99"], mq["flip_mean"]) > (m["flip_p99"], m["flip_mean"]):
                        m = mq; m["output"] = q
            tmp.unlink()
            m["scenario"] = s["sc"].name
            if worst is None or (m["flip_p99"], m["flip_mean"]) > (worst["flip_p99"], worst["flip_mean"]):
                worst = m
        return worst

    def _analyze(self):
        an_path = self.dir / "analysis.json"; rg = self.dir / "ranges.json"
        self._eval(self.spv, self.dir / "prof.npy", ["--profile", rg, "--stride", "4"]); (self.dir / "prof.npy").unlink(missing_ok=True)
        r = run([SHADER_IR, "analyze", "--spv", self.spv, "--ranges", rg, "--out", an_path])
        if r.returncode != 0:
            raise SystemExit(f"analyze failed: {r.stdout}{r.stderr}")
        an = json.loads(an_path.read_text())
        cands = [i["id"] for i in an["instructions"] if i["id"] and (i.get("type") or "").startswith(("f32", "vec2<f32>", "vec3<f32>", "vec4<f32>")) and not i["sinks"] and i["rate"] != "const"]
        trans = [i["id"] for i in an["instructions"] if i.get("ext") in ("Exp", "Exp2", "Log", "Log2", "Pow", "Sin", "Cos", "Sqrt", "InverseSqrt") and not i["sinks"] and i["rate"] != "const"]
        # per-site f16 sensitivity relative to the source module's own f32 output
        own = self.dir / "own.npy"; self._eval(self.spv, own); own_q = quantize(self.p.format, np.load(own))
        sens = {}
        for sid in cands:
            tmp = self.dir / "s.npy"; self._eval(self.spv, tmp, ["--f16-sites", str(sid)])
            m = flip_metrics(own_q, quantize(self.p.format, np.load(tmp)), self.kind); sens[str(sid)] = {"flip_p99": m["flip_p99"], "flip_mean": m["flip_mean"]}
        (self.dir / "s.npy").unlink(missing_ok=True); own.unlink(missing_ok=True)
        return sens, trans

    def demote_sites(self, level: str, p99_budget: float) -> list[int]:
        ranked = sorted(self.sens.items(), key=lambda kv: (kv[1]["flip_p99"], kv[1]["flip_mean"]))
        if level == "none": return []
        if level == "zero": return [int(k) for k, v in ranked if v["flip_p99"] == 0.0 and v["flip_mean"] == 0.0]
        frac = {"q4": 0.25, "q2": 0.5, "q1": 1.0}.get(level)
        if frac: return [int(k) for k, v in ranked if v["flip_p99"] <= p99_budget * frac]
        return [int(k) for k, _ in ranked]

def _rejected(spv: Path, sites: list[int], mode: str) -> set[int]:
    if not sites: return set()
    d = spv.parent / "probe"; d.mkdir(exist_ok=True)
    r = run([SHADER_IR, "demote", "--spv", spv, "--out", d / "o.spv", "--sites", ",".join(map(str, sites)), "--mode", mode, "--ops", d / "o.json"])
    return set() if r.returncode == 0 else {int(m) for m in re.findall(r"^\s+%(\d+):", r.stdout + r.stderr, flags=re.M)}

def build_genome(ctx: Context, g: dict, out_dir: Path, p99_budget: float) -> tuple[Path, list, dict]:
    """Compose: source -> exact rewrite -> hoist -> approx -> demote. Returns (spv, ops, extra uniforms)."""
    out_dir.mkdir(parents=True, exist_ok=True)
    cur = ctx.spv; ops = []; extra_u = dict(ctx.extra_u(ctx.p.uniforms) if ctx.extra_u else {})
    if g["exact"]:
        nxt = out_dir / "1_exact.spv"; o = out_dir / "1.json"
        r = run([SHADER_IR, "rewrite", "--spv", cur, "--out", nxt, "--passes", EXACT_PASSES, "--ops", o])
        if r.returncode != 0: raise RuntimeError("rewrite: " + (r.stdout + r.stderr)[-300:])
        ops += json.loads(o.read_text()); cur = nxt
    if g["hoist"]:
        nxt = out_dir / "2_hoist.spv"; o = out_dir / "2.json"; plan = out_dir / "plan.json"
        r = run([SHADER_IR, "hoist", "--spv", cur, "--out", nxt, "--ops", o, "--plan", plan, "--min-ops", "2"])
        if r.returncode != 0: raise RuntimeError("hoist: " + (r.stdout + r.stderr)[-300:])
        planv = json.loads(plan.read_text())
        if planv:
            ids = [str(x["source_id"]) for x in planv]; dump = out_dir / "dump.json"
            _eval(cur, 2, 2, {k: v for k, v in ctx.samplers.items()}, {**ctx.p.uniforms, **extra_u}, out_dir / "d.npy", ["--dump-ids", ",".join(ids), "--dump", dump]); (out_dir / "d.npy").unlink(missing_ok=True)
            vals = json.loads(dump.read_text())
            for x in planv:
                v = vals[str(x["source_id"])]; extra_u[x["member"]] = v if len(v) > 1 else v[0]
            ops += json.loads(o.read_text()); cur = nxt
    if g["approx"] is not None:
        nxt = out_dir / "3_approx.spv"; o = out_dir / "3.json"; rg = out_dir / "ranges.json"
        _eval(cur, ctx.w, ctx.h, ctx.samplers, {**ctx.p.uniforms, **extra_u}, out_dir / "pr.npy", ["--profile", rg, "--stride", "4"]); (out_dir / "pr.npy").unlink(missing_ok=True)
        an_tmp = out_dir / "an.json"; run([SHADER_IR, "analyze", "--spv", cur, "--ranges", rg, "--out", an_tmp])
        an = json.loads(an_tmp.read_text())
        trans = [i["id"] for i in an["instructions"] if i.get("ext") in ("Exp", "Exp2", "Log", "Log2", "Pow", "Sin", "Cos", "Sqrt", "InverseSqrt") and not i["sinks"] and i["rate"] != "const"]
        if trans:
            r = run([SHADER_IR, "approx", "--spv", cur, "--out", nxt, "--ops", o, "--ranges", rg, "--sites", ",".join(map(str, trans)), "--max-rel-err", str(g["approx"])])
            if r.returncode != 0: raise RuntimeError("approx: " + (r.stdout + r.stderr)[-300:])
            aops = json.loads(o.read_text())
            if aops: ops += aops; cur = nxt
    if g["demote"] != "none":
        # site ids refer to the CURRENT module; recompute candidates on it (ids are preserved through the exact/hoist/approx chain for untouched instructions)
        an_tmp = out_dir / "an2.json"; run([SHADER_IR, "analyze", "--spv", cur, "--out", an_tmp]); an = json.loads(an_tmp.read_text())
        live = {i["id"] for i in an["instructions"] if i["id"]}
        sites = [s for s in ctx.demote_sites(g["demote"], p99_budget) if s in live]
        from .demote import close_variable_loads
        sites = close_variable_loads(ctx.shader, [s for s in sites if s not in _rejected(cur, sites, g["mode"])], set(int(k) for k in ctx.sens), g["mode"], spv=cur)
        if sites:
            nxt = out_dir / "4_demote.spv"; o = out_dir / "4.json"
            cmd = [SHADER_IR, "demote", "--spv", cur, "--out", nxt, "--sites", ",".join(map(str, sites)), "--mode", g["mode"], "--ops", o] + (["--group-converts"] if g["mode"] == "f16" else [])
            r = run(cmd)
            if r.returncode != 0: raise RuntimeError("demote: " + (r.stdout + r.stderr)[-300:])
            ops += json.loads(o.read_text()); cur = nxt
    final = out_dir / f"{ctx.shader}.spv"; shutil.copyfile(cur, final)
    return final, ops, extra_u

def genome_id(g: dict) -> str:
    return f"g_{g['source']}_{'x' if g['exact'] else '-'}{'h' if g['hoist'] else '-'}_a{('%g' % g['approx']).replace('e-0','e-').replace('.', 'p') if g['approx'] else 'off'}_d{g['demote']}{'R' if g['mode'] == 'relaxed' else ''}"

def search(shader: str, gpu_budget: int = 10, rounds: int = 2, samples: int = 20, device=None, seed: int = 0) -> dict:
    require(SHADER_IR, "shader-ir")
    rng = random.Random(seed); bud = budgets()
    sources = _sources(shader)
    ctxs = {name: Context(shader, name, spv, meta.get("extra_u"), device=device) for name, spv, meta in sources}
    sc0 = scenarios_for_shader(shader, "train")[0]; p0 = next(x for x in sc0.passes if x.shader == shader)
    tol, tol_src = tolerance_for(shader, p0.name, p0.format, bud); p99_budget = (tol or {}).get("p99_max", 0.05)
    # ---- generation 0: enumerate and predict every genome in the CPU model
    space = [{"source": s, "exact": e, "hoist": h, "approx": a, "demote": d, "mode": "f16"}
             for s in ctxs for e in (False, True) for h in (False, True) for a in APPROX_LEVELS for d in DEMOTE_LEVELS]
    # exact rewrites showed zero effect on this device class (M2): keep them only when combined with hoist (hoist needs unroll)
    space = [g for g in space if not (g["exact"] and not g["hoist"])]
    preds = []
    for g in space:
        gid = genome_id(g); gd = SEARCH / shader / "genomes" / gid
        try:
            spv, ops, extra_u = build_genome(ctxs[g["source"]], g, gd, p99_budget)
        except RuntimeError as e:
            preds.append({"id": gid, "genome": g, "error": str(e)[:200]}); continue
        n_ops = len(ops)
        if n_ops == 0 and g["source"] == "baseline":
            preds.append({"id": gid, "genome": g, "skip": "identity"}); continue
        ctx = ctxs[g["source"]]; members = None
        plan = gd / "plan.json"
        if g["hoist"] and plan.exists() and json.loads(plan.read_text()):
            planv = json.loads(plan.read_text()); pre = gd / "1_exact.spv"; members = {}
            for s_ in ctx.scenes:
                eu = dict(ctx.extra_u(s_["p"].uniforms) if ctx.extra_u else {}); dump = gd / "dump_pred.json"
                _eval(pre if pre.exists() else ctx.spv, 2, 2, s_["samplers"], {**s_["p"].uniforms, **eu}, gd / "d.npy", ["--dump-ids", ",".join(str(x["source_id"]) for x in planv), "--dump", dump]); (gd / "d.npy").unlink(missing_ok=True)
                vals = json.loads(dump.read_text())
                for x in planv:
                    v = vals[str(x["source_id"])]; eu[x["member"]] = v if len(v) > 1 else v[0]
                members[s_["sc"].name] = eu
        m = ctx.predict(spv, uniforms={**ctx.p.uniforms, **extra_u}, extra_members=members)
        preds.append({"id": gid, "genome": g, "spv": str(spv), "ops": n_ops, "extra_u": extra_u, "pred_p99": m["flip_p99"], "pred_mean": m["flip_mean"], "pred_scenario": m.get("scenario"), "within": m["flip_p99"] <= p99_budget and m["flip_mean"] <= (tol or {}).get("mean_max", 1e9)})
    valid = [p for p in preds if "pred_p99" in p]
    # cost prior: more edit ops and lossier genes tend to be faster; measured single-gene results refine this in round 2
    def prior(p):
        g = p["genome"]; return (g["source"] != "baseline") * 3 + (g["demote"] != "none") * 1 + (g["approx"] is not None) * 1 + g["hoist"] * 1 + p["ops"] / 100.0
    within = sorted([p for p in valid if p["within"]], key=lambda p: (-prior(p), p["pred_p99"]))
    chosen = within[:gpu_budget]
    # ---- measure
    measured = {}
    def measure(p):
        g = p["genome"]; gid = p["id"]; ctx = ctxs[g["source"]]
        write_variant(shader, gid, Path(p["spv"]), [], extra={"genome": g, "predicted": {"flip_p99": p["pred_p99"], "flip_mean": p["pred_mean"]}, "n_ops": p["ops"], "extra_uniforms": p["extra_u"]})
        runs = []
        for sc in scenarios_for_shader(shader, "train") + scenarios_for_shader(shader, "holdout"):
            dsc = copy.deepcopy(sc)
            for pp in dsc.passes:
                if pp.shader == shader:
                    eu = dict(ctx.extra_u(pp.uniforms) if ctx.extra_u else {})
                    # hoisted members depend on this scenario's uniforms: recompute from the plan when present
                    plan = Path(p["spv"]).parent / "plan.json"
                    if plan.exists() and json.loads(plan.read_text()):
                        planv = json.loads(plan.read_text()); pre = Path(p["spv"]).parent / "1_exact.spv"
                        src = pre if pre.exists() else ctx.spv   # hoist without the exact pipeline hoists from the source module
                        dump = Path(p["spv"]).parent / "dump_sc.json"
                        _eval(src, 2, 2, ctx.samplers, {**pp.uniforms, **eu}, Path(p["spv"]).parent / "d.npy", ["--dump-ids", ",".join(str(x["source_id"]) for x in planv), "--dump", dump]); (Path(p["spv"]).parent / "d.npy").unlink(missing_ok=True)
                        vals = json.loads(dump.read_text())
                        for x in planv:
                            v = vals[str(x["source_id"])]; eu[x["member"]] = v if len(v) > 1 else v[0]
                    pp.uniforms = {**pp.uniforms, **eu}
            m = measure_variant_derived(shader, gid, sc, dsc, {}, rounds=rounds if (sc.split == "train" or _mobile()) else 1, samples=samples, device=device)
            if not m.get("ok"):
                runs.append({"scenario": sc.name, "error": m.get("error")}); continue
            pp = next(x for x in sc.passes if x.shader == shader); t, src_t = tolerance_for(shader, pp.name, pp.format, bud)
            gates = evaluate_gates(m, shader, sc, t); gs = evaluate_gates(m, shader, sc, None)
            record(m, gates, [{"pass": "search", "class": "composed", "target": 0, "replaced_by": 0, "detail": json.dumps(g)}], src_t, t, extra={"gates_strict": gs, "genome": g, "search": True, "predicted": {"flip_p99": p["pred_p99"]}})
            tt = next(v for v in m["timing"].values() if v["touched"])
            runs.append({"scenario": sc.name, "split": sc.split, "speedup": tt["speedup"], "ci": tt["speedup_ci95"], "flip_p99": max(v.get("flip_p99", 0.0) for v in m["metrics"].values()), "gates": gates, "gates_strict": gs})
        ok = [r for r in runs if "speedup" in r]
        measured[gid] = {"genome": g, "runs": runs, "min_speedup": min((r["speedup"] for r in ok), default=float("nan")), "worst_p99": max((r["flip_p99"] for r in ok), default=float("nan")),
                         "accepted": bool(ok) and all(all(r["gates"].get(k) for k in ("1", "2", "3", "4")) for r in ok), "predicted_p99": p["pred_p99"]}
    for p in chosen:
        measure(p)
    # ---- round 2: single-gene neighbours of the best measured genomes that were not yet measured
    best = sorted(measured.items(), key=lambda kv: -(kv[1]["min_speedup"] if kv[1]["min_speedup"] == kv[1]["min_speedup"] else -1))[:3]
    by_id = {p["id"]: p for p in valid}
    neighbours = []
    for gid, mres in best:
        g = mres["genome"]
        for key, opts in (("demote", DEMOTE_LEVELS), ("approx", APPROX_LEVELS), ("hoist", (False, True)), ("source", list(ctxs))):
            for o in opts:
                if o == g[key]: continue
                n = dict(g); n[key] = o
                if key == "hoist" and o: n["exact"] = True
                if key == "hoist" and not o: n["exact"] = False
                nid = genome_id(n)
                if nid in by_id and nid not in measured and by_id[nid]["within"]:
                    neighbours.append(by_id[nid])
    rng.shuffle(neighbours)
    for p in neighbours[:max(0, gpu_budget // 2)]:
        measure(p)
    out = {"shader": shader, "tolerance": tol, "space": len(space), "predicted": preds, "measured": measured, "gpu_budget": gpu_budget}
    (SEARCH / shader / "search.json").write_text(json.dumps(out, indent=1))
    write_search_report(shader, out); write_shader_report(shader)
    return out

def write_search_report(shader: str, out: dict):
    L = [f"# {shader}: M5 search", "", f"Genome = (source, exact, hoist, approx level, demote level, mode). {out['space']} genomes enumerated and predicted in the CPU model against the ORIGINAL shader; {len(out['measured'])} measured on the device (budget {out['gpu_budget']} + neighbours). Tolerance: {out['tolerance']}.", "",
         "## Measured (Pareto on measured speedup vs measured error)", "", "| genome | predicted p99 | measured worst p99 | min speedup (train) | accepted |", "|---|---|---|---|---|"]
    rows = sorted(out["measured"].items(), key=lambda kv: -(kv[1]["min_speedup"] if kv[1]["min_speedup"] == kv[1]["min_speedup"] else -9))
    for gid, m in rows:
        L.append(f"| {gid} | {m['predicted_p99']:.4f} | {m['worst_p99']:.4f} | {m['min_speedup']*100:+.2f}% | {'yes' if m['accepted'] else 'no'} |")
    L += ["", "## Predicted but not measured (within budget)", ""]
    pv = [p for p in out["predicted"] if "pred_p99" in p and p["within"] and p["id"] not in out["measured"]]
    L.append(f"{len(pv)} genomes; {sum(1 for p in out['predicted'] if 'pred_p99' in p and not p['within'])} predicted over budget; {sum(1 for p in out['predicted'] if 'error' in p)} failed to build.")
    (LAB / "reports" / f"{shader}.search.md").write_text("\n".join(L) + "\n")
