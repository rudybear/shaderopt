# Lab notebook (append-only)

## 2026-09-23
- Brief rewritten to v4: automatic optimizer with exact static, AI semantic and lossy engines; tolerance is an optional pipeline knob. Repo `rudybear/shaderopt` created.
- Toolchain vendored via IGL `deploy_deps.py`; pinned in `lab/TOOLS.md`.

## 2026-09-24
- IGL full build succeeds after X11 and GL/EGL dev packages. Windowed rendering on RTX PRO 6000 works.
- NEGATIVE: IGL `--headless` segfaults; root cause is NVIDIA 580.178.04 crashing on `vkGetPhysicalDeviceSurfaceCapabilitiesKHR` for a headless surface (probe reproduces without IGL). Runner will use no surface at all.
- IGL's `createTimer` is unimplemented on Vulkan; `ITimestampQueries` attached to `RenderPassDesc` is the timing path.
- axiom-compute audited: compute-only, no lifter, no MIR, scalars only, one GLSL.std.450 op. Guardian endorses a lab-local IR on rspirv `dr::Module` (Request 001).
- rspirv 0.12 round trip of a glslang fragment shader: body byte-identical, generator word differs. M1 lift gate is achievable with zero lifter code.
- M0 STOP: awaiting review of `lab/DISCOVERY.md`.

## 2026-09-24 (M1)
- User confirmed Option A, the lab corpus, and installed the Khronos validation layer.
- Corpus: 9 shaders (bloom chain x4, tonemap_aces, color_grade, fxaa, vignette_grain, deferred_lit), 19 scenarios (13 train, 6 holdout), synthetic generators incl. NaN/Inf/denormal and a G-buffer set.
- Runner (C++/IGL, no surface, ITimestampQueries per render pass) runs all 19 scenarios at 1080p/3440x1440 with 0 validation errors. Contract fixes: no `-g0` (reflection needs OpName); runner rewrites uniform-block DescriptorSet to 1 (IGL hardwires sets); `pad*` members optional.
- Timing sanity: per-pass time stable across K=4..32 (tonemap 8.0 -> 7.7 us; K=1 is 9.3 us due to per-pass setup). Start timestamp is BOTTOM_OF_PIPE in Accurate mode, so passes do not overlap.
- A/A noise floor, desktop, clocks unlocked (P0 2625 MHz, no throttle): vignette rel diff -0.09%, CI [-0.30%, +0.00%], CV 0.60%. Locking clocks needs sudo; not done.
- Observation: fxaa_noise 56 us vs fxaa_edges 23 us at the same resolution: the early-out branch dominates cost; noise input defeats it. Good M2 branch-analysis target.
- IGL does not enable shaderFloatControls2 (IGL_PATCHES.md #1); needed for M4 fast-math.
- shader-ir (Rust) built: lift on rspirv dr::Module, fragment interpreter f64/f32/f16, 27 tests. Corpus round trip body-identical for all 9 shaders.
- lift-check v1 (abs tolerance 2e-3) failed 12/16: wrong yardstick. v2 measures error in storage-format code units: 13/16, then found two model gaps.
- NEGATIVE then POSITIVE: sampler weight quantization by truncation made tonemap edges worse (max 101 codes); round-to-nearest 8-bit fixed-point coordinates reproduce NVIDIA exactly (max 0). Adopted as the CPU sampler model.
- lift-check train split: 16/16 OK, max <= 1 code everywhere except vignette_grain (hash amplification, flagged fragile). M1 gates hold on desktop.
- M1 STOP: report in lab/reports/M1.md.

## 2026-09-25 (M2)
- User: proceed through all milestones without pausing; reports still written per milestone.
- A/A noise floors for all 19 scenarios (2 rounds x 2 x 30 samples, clocks unlocked): |rel diff| <= 0.85% everywhere except vignette_gradient at -2.17% this session (0.09% yesterday); CV 0.25% (fxaa) to 2.8% (deferred). The timing gate uses max(2%, floor) per scenario.
- Verified: glslang -g builds have the same instruction body as -V builds (ids shift by one); source lines map by body index.
- shader-ir analysis landed (rate/sinks/ranges/coord kinds/f16-sites, 9 tests). Corpus: fxaa has 180 sink sites of 233 (address 138, control 79): only 40 float sites are lossy candidates. deferred_lit: 44 sinks incl. 8 discard. Blur weights exp(-0.5 x^2/sigma^2) classify as uniform-rate (hoistable), wsum uniform, loop counter const. color_grade: 25 uniform sites (premultiply candidates).
- NEGATIVE (useful): rounding *all* float sites incl. uv to f16 in tonemap gives FLIP p99 0.57 while no single candidate site exceeds 0.023: f16 UVs alone destroy the image at 1080p. Confirms the hard-sink rule; the "all candidates" figure now excludes sinks.
- shader-ir rewrite passes landed (fold/dce/cse/ident/unroll/divconst/powspec/select, 20 tests; exact pipeline bit-identical in f64 on the whole corpus). Blur unroll 63->269 instructions (9 iterations); post-unroll weights are not folded because glslang routes `x=float(i)` through a Function variable: needs store-to-load forwarding (mem2reg-lite) -> backlog. powspec never fires on the corpus (no constant exponents 2/0.5/1/3). select fires only on deferred_lit's || and && conditions.
- Exact canonicalization measured on all 9 shaders: NEGATIVE across the board (|effect| <= 0.2%, CIs include 0). The driver already does fold/cse/unroll/select. Logged for M5 pruning on this device class. All variants bit-identical in f64/f32 CPU models.
- Classification: tonemap and fxaa are entirely f16-safe on candidate sites (FLIP 0); vignette fragile (0.10); color_grade near the mean budget.
- deferred_lit baseline drifted 22.9 -> 30.8 us between sessions; A/B pairs are interleaved so verdicts hold; investigate clocks.
- M2 report: lab/reports/M2.md. Started M4 graph experiments (formats/resolution) on the bloom chain.
- NEGATIVE then fixed: first graph-experiment run showed exactly zero effect for every format/resolution change because make_job pointed the runner at the original scenario TOML; derived scenarios are now serialized into the job. Invalid records purged.
- Graph experiments (bloom chain, whole-chain time): quarter-res intermediates +7.0..8.5% (FLIP 0 on the gradient scene, p99 0.33..0.56 on hdr_edges -> rejected on train); R11G11B10F intermediates +1.0..1.8% within budget (p99 <= 0.024) but below the 10% min_speedup and the 2% gate; RGBA8 for HDR intermediates destroys highlights (p99 0.9) -> rejected. Reports: lab/reports/bloom_hdr_*.graph.md.
- Wrote 8 semantic hypotheses (blur bilinear5 x2, tonemap onepow/lut1d, deferred pcf4, vignette cheaphash, fxaa dir4, color_grade foldmat) with claims and predicted cost/error; `lab hypo` predicts in the CPU model then measures on the GPU.
- Hypotheses measured (desktop, interleaved, budget = defaults + min_speedup 10%):
  - ACCEPTED gaussian_blur_h/v bilinear5: +11.1..11.6% on all 4 bloom scenarios incl. holdout, FLIP p99 <= 0.010 (edges/noise), 0 on smooth. First accepted lossy variants.
  - ACCEPTED deferred_lit pcf4: +11.65%, FLIP p99 0.000 (penumbra change touches < 1% of pixels on this scene).
  - REJECTED fxaa dir4: +29% on noise, +2..3.6% elsewhere, but FLIP p99 0.27..0.29 on edges/noise/ultrawide: the axis-only direction estimate blends the wrong way on diagonals. Claim falsified.
  - REJECTED tonemap lut1d: 53..108% SLOWER (3 dependent texture fetches cost more than the ALU curve on this GPU) and p99 0.97 on NaN/Inf inputs. Claim falsified on desktop; may differ on mobile.
  - NO EFFECT tonemap onepow: driver already fuses. color_grade foldmat: +1.2..2.0% exact (FLIP 0) but below the 2%/10% gates.
  - REJECTED (metric-limited) vignette cheaphash: FLIP 0.34..0.46 because a different noise realization is different pixels; a statistical-equivalence metric would be needed to judge grain rewrites; parked.
- CPU-model predictions track the GPU: blur bilinear5 predicted FLIP p99 0.0071 vs measured 0.0068; fxaa dir4 predicted max 0.219 vs 0.224 on checker; color_grade foldmat predicted max 0.0112 vs 0.0127.
- NEGATIVE (test-design bug): deferred_spheres' shadow map never shadowed anything (occluder depth 0.55+0.3v vs receiver 0.5+0.3v), so pcf4's "lossless" +11.65% was a fetch-count win with no penumbra to judge. Generator fixed (occluder depth 0.3); deferred_lit re-classified and pcf4 re-measured below.
- deferred_lit pcf4 re-measured with real shadows: +11.70%, FLIP p99 0.0268 (within the 0.05 budget), mean small. ACCEPTED under budget; the penumbra is visibly narrower, so it is a budgeted trade, not a free win.
- M3 tonemap_aces on desktop: RelaxedPrecision has no effect (-1..+0.6%, as the brief predicted for desktop drivers); explicit f16 on 32 sites +1.3..1.9% (below gates); the 28 "zero-error" sites alone are slightly slower (converts cost more than they save). Holdout tonemap_extreme exposes f16 overflow (FLIP 0.97) on inputs > 65504: ranges from train scenarios do not protect against holdout extremes; a range guard or per-site exclusion is needed before f16 is safe there. Also: per-site f16 predictions (results only) underestimate the transformed module's error (0 predicted vs 0.026 measured); the predictor now evaluates the demoted module itself.
- Infrastructure: the M3 sweep was killed by the host's low-memory guard after lab/results grew to 47 GB of raw npy/EXR dumps (page cache). Fixed: EXR only with LAB_WRITE_EXR=1, raw npy deleted after metrics in every A/B path (PNGs and JSON stay). Sweep restarted, chained with hoist and approx.
- M3 sweep: nothing accepted on desktop; RelaxedPrecision ignored; f16 <= 2%; holdout NaN/Inf exposes f16 overflow. Report lab/reports/M3.md.
- M4: hoist exact but <= 1.5%; approx only fits the blur exp and is slower; LUT slower; formats/resolution 1..8% chain. Report lab/reports/M4.md. Fast-math blocked on IGL float_controls2.
- Fixed: demotion sets now closed over Function-variable loads (all-loads-or-none rule).
- M5 search started (8 GPU measurements + neighbours per shader; every genome predicted in the CPU model first).
- Session was cut off while evicting page cache with a 120 GB allocation (do not do that again). Search restarted per shader.
- M5 blur_h first pass: best genome = bilinear5 alone (+11.17%, p99 0.0071, ACCEPTED); every f16 genome measured p99 0.97 on bloom_extreme (holdout overflow) although the train-only prediction said 0.0071 -> predictions now cover every scenario incl. holdout before spending GPU.
- BUG fixed: hoist appended a member whose vector type was declared after the block struct (spirv-val: "requires a previous definition"); 48 of 144 blur genomes failed to build because of it. The pass now moves the member type (and its scalar) ahead of the struct. bilinear5 + hoist: 122 -> 54 instructions.
- M5 search (desktop, 8 measured + neighbours per shader): no composition beats the single best gene; bilinear5 (+hoist) for blur is the only accepted family. Search predictions missed f16 overflow on bloom_extreme because the CPU quantization kept NaN (masked) where the GPU 8-bit store writes 0: fixed in formats.quantize; bloom-chain and tonemap searches re-run.
- Final report: lab/reports/FINAL.md.
- Predictor closed: one-sided NaN/Inf now counts as FLIP 1.0 and predictions chain through downstream judged outputs. Blur search re-run: 120/142 genomes excluded by prediction (bloom_extreme), 11/11 measured genomes accepted (+10.9..+11.2%), predicted == measured p99. The other shaders' search.json files still carry the older predictor's "within" statistics; their measured verdicts are unaffected.

## Handoff (2026-09-25, end of desktop phase)
- State: M0-M5 complete on desktop; reports in lab/reports/ (M1..M4, FINAL, per-shader, classification, canonical, graph, search). Repo pushed.
- Accepted: gaussian_blur_h/v bilinear5 (+11%), deferred_lit pcf4 (+11.7%, budgeted). Everything ALU-side is within noise on the RTX PRO 6000.
- Next: mobile devices (Android headless runner via adb, iOS host), user's real shader corpus, IGL float_controls2 patch (fast-math), upstream proposals 001-P1..P5 via the guardian, backlog in FINAL.md §5.
- Housekeeping: two earlier commits contain ~500 MB of reference npy files (now untracked, .git is 327 MB); rewrite history if clone size matters. Never evict page cache with a huge allocation in this harness.
- Debug (.g.spv) builds do not round-trip byte-identically: rspirv re-emits an OpLine that glslang places before OpFunction inside the function (OpLine %1 14 18 moved). Measured -V builds are unaffected; `lab verify` now excludes .g.spv from the round-trip check.
- Corpus search re-run with the validated predictor: predictor excludes 54..120 genomes per shader before GPU time; predicted == measured p99 on all measured genomes. deferred_lit: pcf4 + exact + hoist + f16 demotion = +14.0% at p99 0.043 (pcf4 alone +11.7%): the first composition that beats its single gene; provisional because deferred_spheres has no holdout scene (add one: extreme values + different light directions).

## 2026-09-28 (Android)
- User connected a Pixel 9 Pro XL (Tensor G4, Mali, Android 17). adb via platform-tools (no sudo) + udev rule for vendor 18d1 (sudo). NDK r27c installed. Cross-build of the runner and an adb job path are being built.

## 2026-09-28 Android runner (adb)
- Cross-compiled shaderlab-runner for arm64-v8a with NDK r27c (`lab/runner/build-android.sh`, static libc++, IGL Vulkan-only, link log+android): builds and links unchanged sources; `sysinfo.cpp` gained an `__ANDROID__` branch (dumpsys thermalservice / dumpsys battery / Mali or kgsl clock sysfs); runner gained `--info` and a fallback to the only integrated GPU when no discrete one exists.
- `lab/tools/labtool/android.py`: `lab android devices|push|probe|clean`, `run_job_android` (push job, run over adb shell, pull result, delete job; inputs cached on the device by sha256), `--android SERIAL` on run/baseline/aa/lift-check/verify; thermal gate (ThermalStatus >= SEVERE: 30 s cooldown, 3 retries, `state.throttled`), throttled samples dropped in measure_variant and aa. CONTRACTS.md "Android runner".
- Verified on Pixel 9 Pro XL (Tensor G4, Mali-G715, Android 17, driver v1.r54p3, Vulkan 1.4.343): shaderFloat16, storageBuffer16BitAccess, shaderFloatControls2 all supported (IGL enables the first two), timestamps supported with period 40.69 ns. Selftest on device: rgba32f max abs err 0, srgb 4.1e-3, ubo 2.4e-4, g0 expected failure; copy pass 95..140 us at 256x128 with the GPU clock reading 150 MHz (idle) before and after. `lab run vignette_gradient --android` end to end: 1769 us median at 1080p, 3 samples.
- Not verified: thermal gate retry path (the phone stayed at ThermalStatus 0), Adreno sysfs paths (no Adreno device), the timing noise floor on the phone (`lab aa` not run; no timing sweeps yet).
- Desktop selftest still PASS after the runner refactor.

- Pixel first A/A (warmup 10, K=8): CV 12%, vignette A/B -9.7%: the Mali clock ramped 150 -> 467 MHz during the samples. Runner now records the GPU clock per sample; stats keep steady-clock samples (top clock within 5%).
- Pixel second A/A (warmup 20, K=16, steady @940 MHz): blur_h/v CV 2.7% (fine), but full-res passes (threshold, composite, vignette) CV 13..19%. They read a 33 MB RGBA32F input per frame at 1080p (~41 GB/s) and are memory-bound; memory-bus DVFS (devfreq_mif, unreadable via sysfs) is the likely source. Actions: per-input texture formats (RGBA16F/RGBA8, as a real app would use) and a screen-off A/A to test the display's bandwidth share.
- NEGATIVE: screen off (dozing) does not reduce the Pixel's full-res variance (vignette CV 18%, threshold/composite 9..10%). Raw series show a stable floor (~1030 us, sometimes ~976 us: two memory-clock states) with bursts 30..60% above it. Added: plateau statistic (samples within 5% of the run minimum, Android only, raw median kept) and a round-paired speedup estimator (B,V pairs cancel slow state drift); gate 4 uses the paired CI on mobile.
- Inputs now carry texture formats (RGBA16F for HDR scenes, RGBA8 for LDR, R16F shadow map); the runner uploads in that format and gen_inputs quantizes identically for the CPU model. All desktop numbers before this point were measured with RGBA32F inputs; the accepted variants are being re-measured on desktop and the phone A/A repeated with the paired protocol.
- deferred_spheres lift-check with f16 shadow coordinates: 4 of 994k pixels differ by up to 21 codes at a checker edge (PCF tap flips at the compare threshold): per-shader max_codes 32 in lift_tolerances.toml, documented.
- Desktop re-measured with app-realistic input formats: blur bilinear5 +10.9..11.4% (p99 <= 0.0101), deferred pcf4 +20.7% (p99 0.0269; up from +11.7% with RGBA32F inputs, the R16F shadow map makes fetch count matter more). All still accepted.
- Pixel with app-realistic inputs (K=16): passes drop to 150..450 us, and the governor no longer reaches 940 MHz (steady at 419..649 MHz, vignette only 3 samples at 467): each submit is too short to keep the GPU busy. Paired CIs +-15..25%: useless. Retrying with K=64 (20..30 ms of GPU work per sample).
- K=64 on the Pixel: chain passes (blur, composite) reach A/A diff |0.2..0.4|% with CV ~4%, but the clock still sits below 940 MHz most of the time (only 4/90 samples at the top): the submit-then-wait loop leaves the GPU idle between samples. Pipelined submits (N in flight) are being added to the runner.
- Pipelined runner (3 in flight) landed; the Pixel was unplugged before its A/A could run. User connected a Samsung Tab S10 Ultra (Dimensity 9300, Mali-G720 Immortalis MC12, Vulkan 1.3.247, timestamp period 76.9 ns) instead: second mobile GPU class. A/A running on it.
- Tab S10 Ultra A/A (3 in flight, K=32, warmup 20, 6x2x15): plateau diffs 0.3..1.5%; CV 1.2..1.5% on blur/grade, 6..11% on vignette/threshold (bursts, plateau filter handles them). GPU clock not readable (SELinux). Usable: gate = max(2%, floor). Full mobile pipeline launched on it (baseline, lift-check, hypo, demote, hoist, graph).
- Tab S10 Ultra (Mali-G720) results, first pass: lift-check bit-exact/1-code vs the CPU model (model holds across vendors). blur bilinear5 +19..31% (accepted), deferred pcf4 +24% (accepted), tonemap 1D LUT +9..11% on Mali (60% SLOWER on desktop: the first platform-specific verdict) but fails on NaN/Inf (needs a guard); fxaa dir4 quality-rejected as on desktop; tonemap f16 demotion "zero" set accepted on 3/5 scenarios and RelaxedPrecision on 2/5: the mobile driver honors it, as the brief predicted. The tablet disconnected during the blur demotion; hoist/graph/search pending reconnection.
