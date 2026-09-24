# shaderopt — project rules for agents

Read `AXIOM_SHADER_LAB.md` (the brief) and `lab/CONTRACTS.md` (file formats) before touching anything. `lab/DISCOVERY.md` records the platform facts; `lab/NOTEBOOK.md` is the append-only log.

## Hard rules
1. `~/sources/axiom-compute` and `~/sources/igl` are read-only. AXIOM concerns go through `FEATURE_REQUEST.md` + the `axiom-guru` skill (`lab/AXIOM_REQUESTS.md`). IGL needs go on a branch as upstreamable patches listed in `lab/IGL_PATCHES.md`.
2. Never modify `lab/shaders/*.frag` once a baseline exists. Variants live in `lab/variants/<shader>/<variant-id>/`.
3. Never fabricate or simulate a measurement. CPU-model numbers are `predicted`; malioc/RGA numbers are `proxy`. If a device or tool is missing, stop and say so.
4. Hard-sink sites (UVs, texture coordinates/indices, branch conditions, discard, float→int) get zero budget without a logged endorsement from the user.
5. Tolerance is opt-in (`lab/budgets.toml` `allow_lossy`). Without it, only exact rewrites and within-noise variants are accepted.
6. Use the pinned tools in `lab/TOOLS.md`, never system `glslc`, for anything measured.
7. Before every commit: `./lab/lab verify`, `cargo test` in `lab/crates/shader-ir`, and the runner selftest.

## Layout
- `lab/lab` CLI (`build`, `gen-inputs`, `run`, `baseline`, `aa`, `lift-check`, `verify`), implemented in `lab/tools/labtool/`.
- `lab/runner/` C++ IGL headless runner. `lab/crates/shader-ir/` Rust lift + interpreter.
- `lab/scenarios/*.toml`, `lab/shaders/*.frag`, `lab/budgets.toml`, `lab/results.jsonl`, `lab/reports/`.

## Conventions
- Milestones marked [STOP] in the brief end with a report, a commit, and waiting for review.
- Every result records tool versions, the IGL commit, device fingerprint and thermal/clock state.
- One job at a time per device. Lock desktop clocks for timing runs (`nvidia-smi -lgc`), reset after (`-rgc`).
