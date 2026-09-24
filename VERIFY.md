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

# 2. Accepted variants (none yet)
```
Accepted variants are listed below with the exact command that reproduces their gate results.
