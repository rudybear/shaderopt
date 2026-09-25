# AXIOM Shader Lab: final report (desktop phase)

Date: 2026-09-25. Device class measured: desktop, NVIDIA RTX PRO 6000 Blackwell, driver 580.178.04, Vulkan 1.4.312. No mobile device was available; every Android/iOS item in the brief is built but unmeasured. Reproduce everything from `VERIFY.md`.

## 1. What was built

| piece | where | state |
|---|---|---|
| Playback sandbox: headless IGL Vulkan runner, per-pass GPU timestamps, reflection-driven bindings, npy readback, validation capture, device state | `lab/runner/` | done, selftest, 19 scenarios at 1080p/3440x1440 |
| Shader IR on rspirv `dr::Module` (lift, side tables), fragment interpreter (f64/f32/f16, quads for derivatives, 8-bit round-to-nearest sampler model) | `lab/crates/shader-ir/` | done, 78 tests |
| Analyses: rate, hard sinks, ranges, sampler coordinate provenance, source lines, per-site f16 sensitivity | `shader-ir analyze/eval --profile/--f16-sites` | done |
| Typed edit-ops: fold/dce/cse/ident/unroll/divconst/powspec/select (exact, ulp), demote relaxed/f16 (lossy), hoist (exact), approx (lossy) | `shader-ir rewrite/demote/hoist/approx` | done |
| AI hypothesis step: semantic rewrites as GLSL variants with claims, predicted in the CPU model, measured, gated | `lab/hypotheses/`, `lab hypo` | done, 8 hypotheses |
| Orchestration: build, inputs, baselines, A/A floors, lift check, classify, canon, graph, hypo, demote, hoist, approx, search, reports | `lab/lab` | done |
| Guardian process for axiom-compute | `FEATURE_REQUEST.md`, `lab/AXIOM_REQUESTS.md` | Request 001 answered: lab-local IR (Option A); 5 upstream proposals parked |

## 2. Measurement discipline that held
- A/A noise floors per scenario and session: 0.1..0.9% (one 2.2%); gate = max(2%, floor, budget min_speedup 10%).
- Every variant interleaved with the baseline (B,V,B,V), median with bootstrap 95% CI, judged on train **and** holdout, on every downstream output, with the Khronos validation layer on.
- CPU model faithful: round trip body-identical for all shaders; CPU f32 vs GPU within one storage code unit everywhere (16/16), after two documented model corrections (sampler fixed-point rounding; FMA contraction accepted as device behaviour).
- Predictions tracked measurements once the storage model was right (blur bilinear5 predicted p99 0.0071, measured 0.0071; tonemap f16 q2 0.0228 vs 0.026). Three prediction gaps were exposed by the holdout scenarios and closed: f16 overflow on inputs > 65504 (predict on every scenario, not just the first train one), NaN-to-black conversion in 8-bit stores (modelled in the quantizer), and one-sided NaN/Inf pixels being masked instead of counted (now maximal error, and predictions chain through the downstream judged passes). Validation on the blur search: 120 of 142 genomes are now excluded by prediction before any GPU time, and all 11 genomes that were measured were accepted with predicted = measured FLIP p99 (0.0071).

## 3. Results per device class (desktop)

**Accepted variants** (all four gates on every train and holdout scenario, budget FLIP mean 0.01 / p99 0.05, min speedup 10%):

| shader | variant | speedup (min over scenarios) | worst FLIP p99 | kind |
|---|---|---|---|---|
| gaussian_blur_h | bilinear5 (AI hypothesis; also with exact+hoist) | +11.1% | 0.0071 | semantic, lossy |
| gaussian_blur_v | bilinear5 | +11.4% | 0.0100 | semantic, lossy |
| deferred_lit | pcf4 (AI hypothesis) | +11.7% | 0.0268 | semantic, lossy |

**Conclusively rejected or ineffective on this device** (each with measured evidence in `lab/results.jsonl` and the per-shader reports):
- Exact canonicalization (fold, cse, unroll, select): 0.0 +/- 0.2% on all 9 shaders. The driver already does it.
- Precision demotion: `RelaxedPrecision` ignored; explicit f16 at most +1.9%, often slower (converts); overflows on the NaN/Inf holdout.
- Uniform hoisting to the CPU: exact, at most +1.5% (color_grade).
- Polynomial approximation: only the blur's `exp` fits at 1e-3; -0.3%. `pow` with small bases cannot be fit at degree <= 7.
- 1D LUT tonemapper: 53..108% slower; fails on NaN/Inf.
- FXAA axis-only direction: +29% on noise, +2..4% elsewhere, but FLIP p99 0.27..0.29: falsified.
- Intermediate formats: R11G11B10F bloom +1.0..1.8% chain (within budget, under gates); RGBA8 destroys HDR.
- Resolution scaling: quarter-res bloom +7..8.5% chain, fails on edges (p99 0.33..0.56).
- Grain hash rewrite: FLIP cannot judge a different noise realization; needs a statistical metric (parked).

**Pareto frontiers** per shader are in `lab/reports/<shader>.md`; the M5 search enumerated 96..144 composed genomes per shader in the CPU model and measured 12 per shader on the device. No composition beat the single best gene on desktop: on this GPU the wins are algorithmic (fewer texture fetches), and ALU-side transforms are within noise.

## 4. Proposals for the app (nothing applied)
1. Bilinear-assisted 5-tap Gaussian blur (both directions): +11% per pass at FLIP p99 <= 0.01.
2. 2x2 bilinear-weighted PCF instead of 3x3: +11.7% at p99 0.027 (visibly narrower penumbra; a budgeted trade).
3. Quarter-resolution bloom intermediates when content is smooth (+8% chain), or R11G11B10F intermediates (+1..2%, safe).
4. Carry the f16, hoisting and format variants to the mobile devices; they are expected to matter on tilers with fp16 ALU rate, where this desktop verdict does not transfer.

## 5. Open items and handoff
- Mobile: no devices. Android headless runner (adb) and iOS host are unbuilt; SPIRV-Cross to MSL untested. The job/result bundle contract is device-agnostic.
- IGL: `VK_KHR_shader_float_controls2` not enabled (`lab/IGL_PATCHES.md` #1); fast-math (M4) blocked until then.
- axiom-compute: five parked upstream proposals (`lab/AXIOM_REQUESTS.md` 001-P1..P5) once device evidence justifies each; the lab stays merge-compatible (same rspirv pin, tolerance grammar, verdict set).
- Clocks were never locked (sudo); the noise floor was low enough that it did not matter.
- Backlog: store-to-load forwarding pass (post-unroll constant folding), pow with uniform exponents in `approx`, statistical-equivalence metric for noise, range guards for f16 in the search, deferred_lit baseline drift investigation (22.9 vs 30.8 us between sessions).
- The user-supplied shader corpus is still pending; the lab corpus (`lab/shaders/`) served M1..M5.
