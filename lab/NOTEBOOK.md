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
