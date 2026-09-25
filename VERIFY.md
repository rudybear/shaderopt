# VERIFY.md — how to re-check every accepted result

Run from the repo root. Each command re-derives its result from scratch on the device it names.

```bash
# 0. toolchain and smoke
./lab/lab build                      # compile + spirv-val the corpus with pinned glslang
./lab/lab verify                     # runner smoke on one scenario + round-trip of every shader
(cd lab/crates/shader-ir && cargo test --release)
bash lab/runner/selftest/run_selftest.sh

# 1. M1 gates (desktop)
./lab/lab lift-check --split train   # round trip body-identical; CPU f32 vs GPU within tolerance per pass
./lab/lab aa bloom_hdr_gradient      # A/A noise floor for the session (repeat per session)
./lab/lab baseline                   # baseline timings + images for every scenario, appended to lab/results.jsonl

# 2. M2: classification and exact canonicalization
./lab/lab classify                   # lab/analysis/<shader>.json + lab/reports/<shader>.classification.md
./lab/lab canon                      # exact rewrites: CPU-verified bit-identical, GPU A/B -> <shader>.canonical.md

# 3. Hypotheses (AI step), demotion (M3), graph experiments (M4)
./lab/lab hypo                       # every lab/hypotheses/<shader>/<id>: predict in the CPU model, measure, gate
./lab/lab demote                     # per-site f16/relaxed demotion sets from the classification's sensitivity
./lab/lab graph --split train        # intermediate formats and resolution on multi-pass scenarios
./lab/lab report                     # regenerate lab/reports/<shader>.md (Pareto frontier, accepted, rejected)
```

## Accepted variants (desktop, RTX PRO 6000, driver 580.178.04, budget = lab/budgets.toml defaults)

| shader | variant | speedup (train min) | worst FLIP p99 | reproduce |
|---|---|---|---|---|
| gaussian_blur_h | hyp_bilinear5 | +11.1% | 0.0071 | `./lab/lab hypo --shader gaussian_blur_h --id bilinear5` |
| gaussian_blur_v | hyp_bilinear5 | +11.4% | 0.0100 | `./lab/lab hypo --shader gaussian_blur_v --id bilinear5` |

```
Accepted variants are listed below with the exact command that reproduces their gate results.
