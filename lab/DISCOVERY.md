# M0 Discovery

Date: 2026-09-24. Host: Linux 6.17, RTX PRO 6000 Blackwell (driver 580.178.04, Vulkan 1.4.312). Status: desktop-only. No mobile devices, no Mac, no shader corpus from the app yet.

## 1. Shaders and pass chains

**Supplied shaders:** none yet. The user will provide samples along the way.

**IGL starter corpus:** 29 render sessions build for Vulkan. Their fragment shaders are trivial: every fullscreen or textured session is `texture(tex, uv)` optionally multiplied by a vertex color (TQSession, ColorSession, YUVColorSession, CheckerboardMipmapSession, TextureViewSession). MRTSession and TQMultiRenderPassSession exercise multiple attachments and passes but with the same trivial math. There is nothing to optimize in them; they are useful only as harness smoke tests and as the vertex-stage / fullscreen-quad template.

**Decision:** build a small lab-owned post-processing corpus under `lab/shaders/` for M1 through M3, written from scratch so there is no licensing question, each a single fullscreen pass with a documented pass chain:

| Shader | Why it is in the corpus |
|---|---|
| `gaussian_blur_h` / `gaussian_blur_v` | separable 9-tap blur; fusion candidate, bilinear-assisted-tap candidate, f16 candidate |
| `bloom_threshold` + blur + `bloom_composite` | 3-pass chain with an HDR intermediate; format and resolution-scaling candidates |
| `tonemap_aces` | `pow`/`exp`-heavy curve; polynomial and LUT refit candidate; gradient banding risk |
| `color_grade` | 3x3 matrix, saturation, lift/gamma/gain; premultiply and uniform-hoisting candidate |
| `fxaa` | branchy, luma-based; branch-reduction and hard-sink (UV) coverage |
| `vignette_grain` | cheap; noise, `smoothstep`; tolerance-gate coverage at the low end |

When the user supplies real shaders they replace this corpus as the primary target; the lab corpus stays as regression coverage.

## 2. AXIOM's real state

Build: `cargo build --release` succeeds; `target/release/axc` present. Tests: `cargo test --release` gives 555 passed, 0 failed, 2 ignored (GPU-gated). Detailed audit against the shader profile: see section 2.1 (filled from the read-only audit).

### 2.1 Audit result (read-only, 2026-09-24)

axiom-compute is a compute-only compiler and does not contain the pieces the brief assumed:

| Brief assumption | Reality | Evidence |
|---|---|---|
| Lift SPIR-V into AXIOM MIR | No SPIR-V input path; rspirv is emit-only. No MIR exists; the IR is HIR, a typechecked tree tied to `.axc` syntax | `crates/axc-codegen/src/emit.rs`; `grep -rni mir crates/` empty |
| Vector/matrix types | Scalars only. `matrix[...]` is KHR cooperative-matrix, no element access | `crates/axc-hir/src/ty.rs:16`, `coopmat.rs` |
| GLSL.std.450 ops | One instruction, `Exp`. Import mechanism is clean and reusable | `crates/axc-hir/src/ext_inst.rs:29`, `codegen/src/body.rs:233` |
| Structured control flow | Present for compute: selection/loop merge, phi for coopmat accumulators; scalars are load/store variables, not SSA | `codegen/src/body.rs:963,1380,1267` |
| Fragment stage, samplers, derivatives, discard | None. `GLCompute` hardcoded; no `Output` storage class, no `OpTypeImage` | `codegen/src/emit.rs:529` |
| `@equiv_fp_tol`, `@rate`, `@range`, `@tolerance`, `@sink` | `@equiv_fp_tol` is not parsed; the others do not exist. Tolerance lives in the driver's `TolerancePolicy` | `hir/src/lower.rs:85-215`, `DESIGN.md:1429`, `driver/src/rewrite_verify.rs:55` |
| Pass/rewrite infrastructure | None. No folding, CSE, DCE; spirv-opt never runs | `codegen/src/emit.rs:51` |
| CPU emulation | None. GPU-vs-GPU differential plus hand-written oracles | `driver/src/rewrite_verify.rs:701` |
| f16 arithmetic | Storage only; an f16 literal is a codegen error | `codegen/src/body.rs:1609` |

What is reusable as-is: the `@strategy { ?hole }` semantics and deterministic variant IDs (`axc-optimize`), the `TolerancePolicy` and `verify_rewrite` verdict schema (`PASS|FAIL|REJECT|SKIPPED|NONDETERMINISTIC_ORACLE|ERROR`), the MCP tool shapes, the GLSL.std.450 import mechanism, and the project's measurement discipline (resident GPU timestamps, min-of-N, no mocked GPU runs).

Process: any change to axiom-compute goes through its 7-agent pipeline (Architect, two design reviews, Coder, QA, two code reviews) with GPU-measured evidence. The lab's intake path is `FEATURE_REQUEST.md` plus the `axiom-guru` skill.

**Recommendation.** Build the lab's shader IR locally on rspirv's `dr::Module`, which already is a 1:1 result-ID-preserving representation with load and assemble, plus a fragment-profile CPU interpreter and pass framework in a lab crate. Adopt axiom-compute's tolerance policy, verdict schema and hole semantics so the two stay merge-compatible. Propose upstream, through the guardian and one at a time, only the pieces that generalize: GLSL.std.450 emitters, a vector type, f16 arithmetic, and the annotation vocabulary. The alternative, growing a shader profile inside axiom-compute before M1 can start, is many milestones with no consumer but the lab and contradicts axiom-compute's own scope statement. The consolidated request is in `FEATURE_REQUEST.md`; the guardian's verdict is recorded in `lab/AXIOM_REQUESTS.md`.


## 3. Tooling

Pinned in `lab/TOOLS.md`. Everything is vendored through IGL's `deploy_deps.py` or built from that vendored source: glslang 15.3.0, SPIRV-Tools v2025.2, SPIRV-Cross 1.4.321, SPIRV-Reflect 1.4.304, FLIP in `.venv`. Gaps: the Khronos validation layer is not installed (acceptance gate 2 blocked until then); malioc, RGA, adb, NDK, Xcode and renderdoc absent (mobile-only).

## 4. Devices

Desktop, from `vulkaninfo`:

| Feature | Value |
|---|---|
| shaderFloat16 | true |
| storageBuffer16BitAccess / uniformAndStorageBuffer16BitAccess | true |
| storageInputOutput16 | false |
| VK_KHR_shader_float_controls2 | present |
| shaderRoundingModeRTEFloat16 / shaderSignedZeroInfNanPreserveFloat16 | true |
| shaderDenormFlushToZeroFloat16 | false |
| timestampComputeAndGraphics | true |
| timestampPeriod | 1 ns |

Consequences: explicit f16 arithmetic and 16-bit storage are testable on desktop; f16 varyings are not (`storageInputOutput16` false), so f16 stays inside the fragment shader. `FPFastMathMode` via float_controls2 is testable. Lavapipe is present and can run the harness for CPU-only sanity, never for timing.

Mobile (added 2026-09-28): **Google Pixel 9 Pro XL** (komodo), Android 17 (SDK 37), Tensor G4, Mali GPU (`ro.hardware.vulkan=mali`), serial `47271FDAS002PF`, reachable via adb (platform-tools 1.0.41, udev rule for vendor 18d1). Runner probe: **Mali-G715**, driver `v1.r54p3-00eac0`, Vulkan 1.4.343, timestamp period **40.69 ns** (coarse: a 1080p pass of ~1.8 ms spans ~44k ticks, fine; K back-to-back executions per sample keep short passes above resolution). Features: shaderFloat16 true, storageBuffer16BitAccess true, uniformAndStorageBuffer16BitAccess true, storageInputOutput16 true, shaderFloatControls2 true (IGL enables only shaderFloat16 and 16-bit storage), timestampComputeAndGraphics true. Runner selftest on the device: RGBA32F max abs error 0.0, sRGB 4.1e-3, UBO 2.4e-4. First 1080p vignette pass: ~1.77 ms (clock at idle 150 MHz before warmup; the Mali clock ramps to 940 MHz under load and is recorded per run). Thermal via `dumpsys thermalservice` (status 0 at connection), GPU clock via `/sys/class/misc/mali0/device/` (see `lab/TOOLS.md`). iOS: none.

## 5. IGL

- Pinned commit `6804e57c74bba676168a30bba20f976567240e1a`; full desktop build with Vulkan, shell, samples and IGLU succeeds after the user installed the X11 and GL/EGL dev packages. Windowed sessions render on the RTX PRO 6000.
- **Timestamps.** IGL's older `IDevice::createTimer` API is not implemented by the Vulkan backend (that is what `GPUTimerSession` logs). The supported path is `ITimestampQueries` (`src/igl/TimestampQueries.h`): create with `createTimestampQueries(maxSlots)`, attach to a render pass through `RenderPassDesc::timestampQuery.queries`, and the Vulkan encoder writes the start timestamp before `vkCmdBeginRenderPass` and the end after `vkCmdEndRenderPass` (`src/igl/vulkan/RenderCommandEncoder.cpp:262,355`). The query-pool reset is recorded lazily and must happen outside a render pass, which the encoder path guarantees. This matches the brief's "timestamps outside render passes" rule with no IGL change.
- **shaderFloat16 / 16-bit storage.** `VulkanFeatures` (`src/igl/vulkan/VulkanFeatures.cpp`) carries the `VkPhysicalDeviceShaderFloat16Int8Features` and 16-bit-storage structs; they are enabled at device creation when the device supports them. To be verified in M1 by reflecting the enabled features from the created device.
- **Headless.** IGL's shell `--headless` uses `VK_EXT_headless_surface` plus a swapchain. NVIDIA 580.178.04 segfaults inside the driver on `vkGetPhysicalDeviceSurfaceCapabilitiesKHR` for such a surface, reproduced without IGL by `lab/probes/headless_surface_probe.c` (llvmpipe is fine). The lab runner therefore creates the device with no window and a zero swapchain size (`HWDevice::create` skips `initSwapchain` when width or height is 0) and renders into offscreen textures with readback. This is a driver bug, not an IGL patch candidate.
- **Reflection.** SPIRV-Reflect source is vendored; the runner compiles it in to derive descriptor bindings and I/O from each SPIR-V variant.

## 6. Plan and gaps

Order of work for M1:
1. `lab/shaders/` corpus with GLSL sources and a scenario per chain.
2. Runner: one C++ executable linking the IGL Vulkan backend headless, driven by a scenario TOML and a job bundle, writing EXR/PNG and per-pass timestamps to a result bundle. Vertex stage is a fullscreen triangle shared across variants.
3. Baseline and A/A noise floor on desktop.
4. Faithful lift: blocked on the AXIOM gaps below; the lab-local fallback is a SPIR-V→MIR lifter and interpreter inside the lab until the guardian process lands the pieces in axiom-compute.

Gaps that need the user:
- Install the Khronos validation layer (sudo).
- Mobile devices and a Mac, when available.
- Real shader samples and input images.

## 7. Guardian verdict and the M0 STOP

The `axiom-guru` guardian reviewed `FEATURE_REQUEST.md` (verdict in `lab/AXIOM_REQUESTS.md`, Request 001): **Option A endorsed**, the lab builds its shader IR locally on rspirv `dr::Module`, axiom-compute stays untouched through M3, and five additive pieces (GLSL.std.450 emitters, vector types, f16 arithmetic, `@equiv_fp_tol` parsing, the `@rate/@range/@tolerance/@sink` family) are parked as individually-welcome upstream proposals pending device evidence and your approval. Evidence that the substrate works: `lab/probes/rspirv_roundtrip` re-assembles a glslang fragment module body-identically, with only the generator word of the header changed.

**Decisions needed from you before M1 starts:**
1. Confirm Option A, or overrule it.
2. Confirm the lab-owned post-processing corpus in §1 as the M1 through M3 target until you supply shaders.
3. Install the Khronos validation layer when convenient (acceptance gate 2).
