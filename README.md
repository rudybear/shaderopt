# shaderopt: a measurement-driven shader optimization lab

shaderopt finds faster versions of fullscreen post-processing shaders and proves them on real GPUs. It lifts the compiled SPIR-V into an editable IR, runs static analysis, exact rewrites, AI-proposed semantic rewrites and lossy transforms on it, predicts the effect of every candidate in a CPU model, and then measures the survivors on the actual devices, interleaved with the baseline, behind a fixed set of acceptance gates. The output per shader and device class is a Pareto frontier of GPU time versus image error and a list of accepted variants with evidence.

Drivers never change what a shader computes; this lab is allowed to, within a quality budget you control. That is where the wins come from: on an RTX PRO 6000 and on a Mali-G720 the accepted variants are algorithmic rewrites (fewer texture fetches) and, on mobile only, precision demotion; every exact compiler-style rewrite measured within noise because the drivers already do them. See `lab/reports/FINAL.md`.

## What is in the box

| piece | path |
|---|---|
| The brief and design | `AXIOM_SHADER_LAB.md` |
| Plugging in your own shaders, images, devices | `docs/ONBOARDING.md` |
| File formats and CLI contracts | `lab/CONTRACTS.md` |
| Headless GPU runner (C++/IGL, desktop Vulkan and Android arm64) | `lab/runner/` |
| Shader IR, interpreter, analyses, transforms (Rust) | `lab/crates/shader-ir/` |
| Orchestration CLI | `lab/lab` (Python, `lab/tools/labtool/`) |
| Shader corpus, scenarios, budgets, hypotheses | `lab/shaders/`, `lab/scenarios/`, `lab/budgets.toml`, `lab/hypotheses/` |
| Reports and the results log | `lab/reports/`, `lab/results.jsonl` |
| How to reproduce every accepted result | `VERIFY.md` |

## Quick start (desktop, Linux, NVIDIA or any Vulkan 1.3 GPU)

Prerequisites: a Vulkan driver, `cmake`, `ninja`, a C++20 compiler, Rust (`cargo`), Python 3.12 with `uv`, and Meta IGL cloned next to this repo (`~/sources/igl`, pinned in `lab/TOOLS.md`) with its dependencies deployed (`python3 deploy_deps.py` inside IGL). IGL's dependency bootstrap vendors glslang, SPIRV-Tools, SPIRV-Cross and SPIRV-Reflect; the lab builds them from that tree (`lab/TOOLS.md` has the exact paths and versions).

```bash
uv venv .venv --python 3.12 && uv pip install --python .venv/bin/python numpy imageio opencv-python-headless OpenEXR flip-evaluator toml
(cd lab/runner && cmake -S . -B build -G Ninja -DCMAKE_BUILD_TYPE=Release && ninja -C build)
(cd lab/crates/shader-ir && cargo build --release)
./lab/lab build            # compile + validate the shader corpus with the pinned glslang
./lab/lab verify           # runner smoke test + round trip of every shader
./lab/lab aa bloom_hdr_gradient   # A/A noise floor for this device and session
./lab/lab baseline         # baseline timings and images for every scenario
./lab/lab classify         # rate, sinks, ranges, sensitivity, pass graph  -> lab/reports/<shader>.classification.md
./lab/lab hypo             # measure every hypothesis in lab/hypotheses     -> lab/reports/<shader>.md
./lab/lab demote           # precision demotion sets (relaxed / explicit f16)
./lab/lab search           # composed edit-op search, predicted then measured
./lab/lab report           # per-shader Pareto frontier, accepted and rejected
```

Android: `./lab/lab android devices`, `./lab/lab android push`, then add `--android SERIAL --inflight 3 --iterations 32 --warmup 20` to any command above. `docs/ONBOARDING.md` has the device setup.

## How a variant is judged

Every variant is compiled to SPIR-V, validated, predicted in the CPU model against the original shader on every scenario (train and holdout), then measured on the device interleaved with the baseline (B, V, B, V). It is accepted for a device class only if all gates hold: validation passes and adds no new messages; quality on every judged output is within the effective tolerance on train and holdout; the speedup exceeds max(2%, the session's A/A noise floor, the budget's minimum) with a bootstrap 95% CI excluding zero. With no tolerance configured only exact rewrites and within-noise variants pass. Nothing is ever applied to your app by the lab; accepted variants are proposals with evidence.

## Status

Desktop (RTX PRO 6000, driver 580) and one Android device class (Samsung Tab S10 Ultra, Mali-G720) are measured; see `lab/reports/`. iOS is designed but unbuilt. axiom-compute integration is through a guardian process (`lab/AXIOM_REQUESTS.md`); the lab's own IR is rspirv-based and merge-compatible with it.

## License

See `LICENSE`. Third-party headers under `lab/runner/third_party/` keep their own licenses (toml++ MIT, nlohmann/json MIT).
