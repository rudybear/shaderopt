"""Per-shader report: Pareto frontier (time vs error) per device, accepted variants with evidence, rejected ideas."""
from __future__ import annotations
import json
from collections import defaultdict
from .paths import LAB, RESULTS_JSONL, VARIANTS

REPORTS = LAB / "reports"

def load_results(shader: str) -> list[dict]:
    if not RESULTS_JSONL.exists():
        return []
    out = []
    for line in RESULTS_JSONL.read_text().splitlines():
        try:
            r = json.loads(line)
        except json.JSONDecodeError:
            continue
        if r.get("shader") == shader and r.get("variant_id") not in (None, "baseline"):
            out.append(r)
    return out

def _err(r: dict) -> float:
    vals = [m.get("flip_p99", 0.0) if m.get("metric") != "exact" else (0.0 if m.get("mismatches", 0) == 0 else 1.0) for m in r["metrics"].values()]
    return max(vals) if vals else 0.0

def _speed(r: dict) -> float:
    sp = [v[0] for v in r.get("speedup", {}).values()]
    return sum(sp) / len(sp) if sp else 0.0

def pareto(points: list[tuple[float, float, dict]]) -> list[dict]:
    """points: (error, speedup, rec). Frontier = no other point has <= error and >= speedup with one strict."""
    front = []
    for e, s, r in points:
        dominated = any((e2 <= e and s2 >= s) and (e2 < e or s2 > s) for e2, s2, _ in points)
        if not dominated:
            front.append(r)
    return sorted(front, key=lambda r: r["err"])

def write_shader_report(shader: str):
    rs = load_results(shader)
    by_dev = defaultdict(list)
    for r in rs:
        by_dev[r["device_fingerprint"]].append(r)
    L = [f"# {shader}", "", f"Every variant measured against the same device's baseline, interleaved. Speedup is the relative reduction of the touched pass's median GPU time with a bootstrap 95% CI. Error is the worst FLIP p99 over the judged outputs. Gates: 1 valid, 2 no new validation messages, 3 quality within the effective tolerance, 4 timing (>= max(2%, noise floor, min_speedup), CI excludes 0). `strict` gates use no tolerance. Variants live in `lab/variants/{shader}/`.", ""]
    for dev, recs in by_dev.items():
        # latest record per (variant, scenario)
        latest = {}
        for r in recs:
            latest[(r["variant_id"], r["scenario"])] = r
        per_variant = defaultdict(list)
        for (vid, sc), r in latest.items():
            per_variant[vid].append(r)
        L += [f"## Device {dev}", ""]
        rows = []
        for vid, vr in per_variant.items():
            train = [r for r in vr if r["split"] == "train"]; hold = [r for r in vr if r["split"] == "holdout"]
            err = max((_err(r) for r in vr), default=0.0); sp = min((_speed(r) for r in train), default=0.0)
            acc = all(all(r["gates"].get(k) for k in ("1", "2", "3", "4")) for r in vr) and bool(vr)
            acc_strict = all(all(r.get("gates_strict", r["gates"]).get(k) for k in ("1", "2", "3", "4")) for r in vr) and bool(vr)
            rows.append((err, sp, {"variant_id": vid, "err": err, "speedup": sp, "n_train": len(train), "n_hold": len(hold), "accepted": acc, "accepted_strict": acc_strict,
                                   "tolerance": vr[0]["tolerance"], "edit_ops": len(vr[0].get("edit_ops", [])), "mode": vr[0].get("mode"), "set": vr[0].get("set"), "sites": vr[0].get("sites")}))
        front = pareto(rows)
        L += ["### Pareto frontier (error vs speedup)", "", "| variant | worst FLIP p99 | min speedup (train) | scenarios (train/holdout) | accepted (budget) | accepted (strict) |", "|---|---|---|---|---|---|"]
        for r in front:
            L.append(f"| {r['variant_id']} | {r['err']:.4f} | {r['speedup']*100:+.2f}% | {r['n_train']}/{r['n_hold']} | {'yes' if r['accepted'] else 'no'} | {'yes' if r['accepted_strict'] else 'no'} |")
        acc = [r for _, _, r in rows if r["accepted"]]
        L += ["", "### Accepted variants", ""]
        if acc:
            for r in sorted(acc, key=lambda x: -x["speedup"]):
                meta = VARIANTS / shader / r["variant_id"] / "variant.json"
                L.append(f"- **{r['variant_id']}**: speedup {r['speedup']*100:+.2f}%, worst FLIP p99 {r['err']:.4f}, tolerance {r['tolerance']['source']}, {r['edit_ops']} edit ops" + (f", sites {r['sites']}" if r.get('sites') else "") + (f" (`{meta.relative_to(LAB.parent)}`)" if meta.exists() else ""))
        else:
            L.append("- none")
        L += ["", "### All variants", "", "| variant | mode | set | sites | worst FLIP p99 | min speedup | accepted | strict |", "|---|---|---|---|---|---|---|---|"]
        for e, s, r in sorted(rows, key=lambda x: (-x[1], x[0])):
            L.append(f"| {r['variant_id']} | {r.get('mode') or ''} | {r.get('set') or ''} | {len(r['sites']) if r.get('sites') else ''} | {e:.4f} | {s*100:+.2f}% | {'yes' if r['accepted'] else 'no'} | {'yes' if r['accepted_strict'] else 'no'} |")
        L.append("")
    out = REPORTS / f"{shader}.md"; out.write_text("\n".join(L) + "\n"); return out
