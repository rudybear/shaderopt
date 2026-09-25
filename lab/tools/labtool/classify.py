"""M2 classification: rate, sinks, ranges, sensitivity, pass graph -> lab/analysis/<shader>.json + lab/reports/<shader>.classification.md"""
from __future__ import annotations
import json
from pathlib import Path
from collections import defaultdict
import numpy as np
from .paths import LAB, SHADER_IR, SPV_DIR, SHADERS, run, require
from .scenarios import Scenario, all_scenarios
from .jobs import make_job, run_job, load_result_images, gen_inputs
from .formats import quantize
from .metrics import flip_metrics, kind_for
from .experiment import budgets, scenarios_for_shader

ANALYSIS = LAB / "analysis"
REPORTS = LAB / "reports"

def _pass_inputs(sc: Scenario, p, res: dict, inputs: dict) -> dict[str, Path]:
    return {s: (inputs[src] if src in inputs else Path(res["_dir"]) / res["images"][src]) for s, src in p.samplers.items()}

def _eval(spv: Path, w: int, h: int, samplers: dict, uniforms: dict, out: Path, extra: list[str], nearest=False, mode="f32") -> str:
    cmd = [SHADER_IR, "eval", "--spv", spv, "--width", str(w), "--height", str(h), "--mode", mode, "--out", out, "--sampler-weight-bits", "8"] + extra
    for n, pth in samplers.items():
        cmd += ["--sampler", f"{n}={pth}" + (":nearest" if nearest else "")]
    for n, v in uniforms.items():
        cmd += ["--uniform", f"{n}={json.dumps(v) if isinstance(v, list) else v}"]
    r = run(cmd)
    if r.returncode != 0:
        raise SystemExit(f"shader-ir eval failed ({spv}):\n{r.stdout}{r.stderr}")
    return r.stdout

def merge_ranges(files: list[Path]) -> dict:
    m: dict = {}
    for f in files:
        for k, v in json.loads(f.read_text()).items():
            if k not in m:
                m[k] = dict(v)
            else:
                a = m[k]; a["min"] = min(a["min"], v["min"]); a["max"] = max(a["max"], v["max"])
                for c in ("nan", "inf", "samples"):
                    a[c] = a.get(c, 0) + v.get(c, 0)
    return m

def classify(shader: str, max_scenarios: int = 2, stride: int = 4, sensitivity: bool = True, device=None) -> dict:
    require(SHADER_IR, "shader-ir")
    ANALYSIS.mkdir(exist_ok=True); REPORTS.mkdir(exist_ok=True)
    scs = scenarios_for_shader(shader, "train")[:max_scenarios]
    if not scs:
        raise SystemExit(f"no train scenario uses {shader}")
    spv = SPV_DIR / f"{shader}.spv"; gspv = SPV_DIR / f"{shader}.g.spv"
    work = ANALYSIS / shader; work.mkdir(parents=True, exist_ok=True)
    range_files, contexts = [], []
    for sc in scs:
        jd = make_job(sc, "baseline", samples=1, iterations=1, warmup=0, tag="classify")
        res, out = run_job(jd, sc, "baseline", device=device)
        if not res.get("ok"):
            raise SystemExit(f"runner failed on {sc.name}: {res.get('error')}")
        inputs = gen_inputs(sc)
        for p in sc.passes:
            if p.shader != shader:
                continue
            w, h = sc.pass_size(p)
            rf = work / f"ranges_{sc.name}_{p.name}.json"
            _eval(spv, w, h, _pass_inputs(sc, p, res, inputs), p.uniforms, work / f"ref_{sc.name}_{p.name}.npy", ["--profile", rf, "--stride", str(stride)], nearest=(p.sampler == "nearest"))
            range_files.append(rf); contexts.append((sc, p, res, inputs))
    merged = work / "ranges.json"; merged.write_text(json.dumps(merge_ranges(range_files)))
    analysis_path = work / "analysis.json"
    r = run([SHADER_IR, "analyze", "--spv", spv, "--debug-spv", gspv, "--ranges", merged, "--out", analysis_path])
    if r.returncode != 0:
        raise SystemExit(f"analyze failed: {r.stdout}{r.stderr}")
    an = json.loads(analysis_path.read_text())
    # ---- sensitivity: round each candidate site to f16 in the CPU model, measure FLIP vs the unperturbed f32 output
    sc, p, res, inputs = contexts[0]
    w, h = sc.pass_size(p); bud = budgets(); kind = kind_for(p.name, p.format, bud)
    ref_path = work / f"ref_{sc.name}_{p.name}.npy"
    _eval(spv, w, h, _pass_inputs(sc, p, res, inputs), p.uniforms, ref_path, [], nearest=(p.sampler == "nearest"))
    ref = quantize(p.format, np.load(ref_path))
    sens = {}
    if sensitivity:
        cands = [i for i in an["instructions"] if i["id"] and i["type"].startswith(("f32", "vec2<f32>", "vec3<f32>", "vec4<f32>")) and not i["sinks"] and i["rate"] != "const"]
        def one(sites: str, key: str):
            outp = work / f"sens_{key}.npy"
            _eval(spv, w, h, _pass_inputs(sc, p, res, inputs), p.uniforms, outp, ["--f16-sites", sites] if sites != "all" else ["--f16-all"], nearest=(p.sampler == "nearest"))
            m = flip_metrics(ref, quantize(p.format, np.load(outp)), kind); outp.unlink(missing_ok=True); return m
        sens["all"] = one("all", "all")
        for i in cands:
            sens[str(i["id"])] = one(str(i["id"]), str(i["id"]))
    # ---- pass graph
    graph = []
    for s in all_scenarios():
        for idx, cp in enumerate(s.passes):
            if cp.shader != shader:
                continue
            for sname, src in cp.samplers.items():
                prod = next((pp for pp in s.passes[:idx] if pp.name == src), None)
                if prod is None:
                    continue
                samp = next((x for x in an["samplers"] if x["name"] == sname), None)
                kinds = sorted({x["coord_kind"] for x in samp["samples"]}) if samp else ["unknown"]
                same_res = s.pass_size(prod) == s.pass_size(cp)
                graph.append({"scenario": s.name, "producer": prod.name, "producer_shader": prod.shader, "consumer": cp.name, "sampler": sname,
                              "coord_kinds": kinds, "same_resolution": same_res, "fusable": same_res and kinds == ["uv_exact"]})
    result = {"shader": shader, "scenarios": [s.name for s in scs], "analysis": an, "sensitivity": sens, "sensitivity_context": {"scenario": sc.name, "pass": p.name, "format": p.format, "metric": kind},
              "pass_graph": graph}
    (ANALYSIS / f"{shader}.json").write_text(json.dumps(result, indent=1))
    write_classification_report(result)
    return result

def write_classification_report(r: dict) -> Path:
    an = r["analysis"]; shader = r["shader"]
    src_lines = (SHADERS / f"{shader}.frag").read_text().splitlines()
    def src(line):
        return src_lines[line - 1].strip() if line and 0 < line <= len(src_lines) else ""
    ins = an["instructions"]
    by_rate = defaultdict(int)
    for i in ins:
        if i["id"]:
            by_rate[i["rate"]] += 1
    sink_kinds = defaultdict(list)
    for i in ins:
        for k in i["sinks"]:
            sink_kinds[k].append(i)
    L = [f"# {shader}: classification", "", f"Scenarios used for ranges: {', '.join(r['scenarios'])} (train). Sensitivity measured on {r['sensitivity_context']['scenario']} / pass {r['sensitivity_context']['pass']} ({r['sensitivity_context']['format']}, metric {r['sensitivity_context']['metric']}), each candidate site rounded to f16 in the CPU f32 model. Source: `lab/shaders/{shader}.frag`; ids are result ids of `lab/build/spv/{shader}.spv`.", ""]
    L += ["## Summary", "", "| | |", "|---|---|"]
    for k, v in an["summary"].items():
        L.append(f"| {k} | {v} |")
    L += ["", f"Rate histogram (results): " + ", ".join(f"{k}={v}" for k, v in sorted(by_rate.items())), ""]
    L += ["## Hard sinks (zero budget)", "", "| kind | sites | source lines |", "|---|---|---|"]
    for k in ("address", "control", "discard", "convert"):
        sites = sink_kinds.get(k, [])
        lines = sorted({i["line"] for i in sites if i["line"]})
        L.append(f"| {k} | {len(sites)} | {', '.join(str(x) for x in lines[:20])}{' ...' if len(lines) > 20 else ''} |")
    L += ["", "Branch conditions and sample coordinates by source line:", ""]
    seen = set()
    for i in ins:
        if i["sinks"] and i["line"] and i["line"] not in seen and any(k in ("control", "discard", "address") for k in i["sinks"]):
            seen.add(i["line"]); L.append(f"- L{i['line']} `{src(i['line'])}` -> {', '.join(sorted(set(i['sinks'])))}")
    L += ["", "## Samplers", "", "| sampler | binding | samples | coordinate kinds |", "|---|---|---|---|"]
    for s in an["samplers"]:
        L.append(f"| {s['name']} | {s['binding']} | {len(s['samples'])} | {', '.join(sorted({x['coord_kind'] for x in s['samples']}))} |")
    L += ["", "## Ranges (from the CPU model over scenario pixels)", "", "Float results with NaN/Inf observed, or ranges beyond f16 (|x| > 65504) or below f16 normal (|x| < 6.1e-5 while nonzero):", ""]
    flagged = 0
    for i in ins:
        rg = i.get("range")
        if not rg or not i["id"]:
            continue
        big = max(abs(rg["min"]), abs(rg["max"])) > 65504
        tiny = 0 < max(abs(rg["min"]), abs(rg["max"])) < 6.1e-5
        if rg["nan"] or rg["inf"] or big or tiny:
            flagged += 1
            if flagged <= 40:
                L.append(f"- %{i['id']} {i['op']}{('/' + i['ext']) if i.get('ext') else ''} L{i['line']} `{src(i['line'])}`: [{rg['min']:.4g}, {rg['max']:.4g}] nan={rg['nan']} inf={rg['inf']}")
    if not flagged:
        L.append("- none")
    sens = r["sensitivity"]
    if sens:
        L += ["", "## Sensitivity to f16 rounding per site (CPU prediction)", "", f"All candidate sites at once: FLIP mean {sens['all']['flip_mean']:.4f}, p99 {sens['all']['flip_p99']:.4f}, max {sens['all']['flip_max']:.4f}.", "",
              "Most sensitive sites (avoid or budget carefully):", "", "| id | op | rate | line | source | FLIP mean | FLIP p99 |", "|---|---|---|---|---|---|---|"]
        byid = {str(i["id"]): i for i in ins}
        ranked = sorted(((k, v) for k, v in sens.items() if k != "all"), key=lambda kv: -kv[1]["flip_p99"])
        for k, v in ranked[:12]:
            i = byid[k]; L.append(f"| %{k} | {i['op']}{('/' + i['ext']) if i.get('ext') else ''} | {i['rate']} | {i['line']} | `{src(i['line'])[:60]}` | {v['flip_mean']:.4f} | {v['flip_p99']:.4f} |")
        zero = [k for k, v in ranked if v["flip_p99"] == 0.0]
        L += ["", f"Sites whose f16 rounding changes no output code at all: {len(zero)} of {len(ranked)} (free demotion candidates): " + ", ".join(f"%{k}" for k in zero[:40]) + (" ..." if len(zero) > 40 else ""), ""]
    L += ["## Pass graph", ""]
    if r["pass_graph"]:
        L += ["| scenario | producer | consumer | sampler | coordinate kinds | same resolution | fusable |", "|---|---|---|---|---|---|---|"]
        for g in r["pass_graph"]:
            L.append(f"| {g['scenario']} | {g['producer']} ({g['producer_shader']}) | {g['consumer']} | {g['sampler']} | {', '.join(g['coord_kinds'])} | {g['same_resolution']} | {'yes' if g['fusable'] else 'no'} |")
    else:
        L.append("This shader never consumes another pass's output in the current scenarios.")
    L += ["", "## Rate by source line", "", "| line | source | const | uniform | pixel |", "|---|---|---|---|---|"]
    per_line = defaultdict(lambda: defaultdict(int))
    for i in ins:
        if i["id"] and i["line"]:
            per_line[i["line"]][i["rate"]] += 1
    for ln in sorted(per_line):
        c = per_line[ln]; L.append(f"| {ln} | `{src(ln)[:70]}` | {c['const']} | {c['uniform']} | {c['pixel']} |")
    out = REPORTS / f"{shader}.classification.md"; out.write_text("\n".join(L) + "\n")
    return out
