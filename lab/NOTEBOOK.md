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
