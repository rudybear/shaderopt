#!/usr/bin/env python
"""check.py <result_dir> <input.npy> <pass> <mode> [--samples N]
mode: exact (tol 1e-6) | srgb (tol 0.01) | ubo (expected = swap(R,G) of input, alpha 1, half-res nearest)"""
import json, os, sys
import numpy as np

result_dir, src_path, pass_name, mode = sys.argv[1:5]
samples = 5
if "--samples" in sys.argv:
    samples = int(sys.argv[sys.argv.index("--samples") + 1])
with open(os.path.join(result_dir, "result.json")) as f:
    r = json.load(f)
print(f"[{mode}] ok={r['ok']} error={r['error']}")
if not r["ok"]:
    sys.exit(1)
src = np.load(src_path)
out = np.load(os.path.join(result_dir, r["images"][pass_name]))
if mode == "ubo":
    # nearest sampling at half resolution picks texel (2x+1, 2y+1)?? no: pixel centers of the
    # half-res grid map to uv = (x+0.5)/(W/2), i.e. between texels 2x and 2x+1 -> nearest rounds to 2x+1
    exp = src[1::2, 1::2, :].copy()
    exp = exp[..., [1, 0, 2, 3]]  # channel swap
    tol = 2e-3  # RGBA16F storage
else:
    exp = src
    tol = 1e-6 if mode == "exact" else 0.01
print(f"output shape={out.shape} expected={exp.shape}")
if out.shape != exp.shape:
    print("FAIL: shape mismatch"); sys.exit(1)
err = np.abs(out - exp)
print(f"max abs error = {err.max():.3e} (tolerance {tol}); mean = {err.mean():.3e}")
print(f"corner samples out[0,0]={out[0,0]} out[-1,-1]={out[-1,-1]} exp[0,0]={exp[0,0]} exp[-1,-1]={exp[-1,-1]}")
ok = err.max() < tol
t = r["timings_ns"][pass_name]
print(f"timings_ns[{pass_name}] = {[round(x, 1) for x in t]}  (us: {[round(x / 1000, 2) for x in t]})")
if len(t) != samples or any(x <= 0 for x in t):
    print(f"FAIL: expected {samples} timings > 0"); ok = False
v = r["validation"]
print(f"validation: errors={v['errors']} warnings={v['warnings']} messages={len(v['messages'])}")
for m in v["messages"]:
    print("  ", m[:300].replace("\n", " | "))
if v["errors"] != 0:
    print("FAIL: validation errors"); ok = False
print("PASS" if ok else "FAIL")
sys.exit(0 if ok else 1)
