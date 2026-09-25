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
