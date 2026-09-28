#!/usr/bin/env python
"""make_job.py <jobdir> <scenario.toml> <input.npy> <pass>=<file.spv> ... [--samples N --iterations K --warmup W --inflight F]"""
import json, os, sys

args = sys.argv[1:]
jobdir, scenario, src = args[0], args[1], args[2]
passes, samples, iterations, warmup, inflight = {}, 5, 4, 2, 3
i = 3
while i < len(args):
    a = args[i]
    if a == "--samples":
        samples = int(args[i + 1]); i += 2
    elif a == "--iterations":
        iterations = int(args[i + 1]); i += 2
    elif a == "--warmup":
        warmup = int(args[i + 1]); i += 2
    elif a == "--inflight":
        inflight = int(args[i + 1]); i += 2
    else:
        k, v = a.split("=", 1); passes[k] = v; i += 1
os.makedirs(jobdir, exist_ok=True)
rel = lambda p: os.path.relpath(os.path.abspath(p), os.path.abspath(jobdir))
job = {
    "schema": 1,
    "scenario": rel(scenario),
    "variant_id": "baseline",
    "passes": {k: rel(v) for k, v in passes.items()},
    "inputs": {"src": rel(src)},
    "samples": samples,
    "iterations": iterations,
    "warmup": warmup,
    "inflight": inflight,
    "readback": "last",
}
with open(os.path.join(jobdir, "job.json"), "w") as f:
    json.dump(job, f, indent=2)
print(json.dumps(job, indent=2))
