# shaderlab-runner

Headless C++ executable on Meta IGL's Vulkan backend. It plays a chain of fullscreen
fragment-shader passes described by a scenario (`.toml`) and a job bundle (`job.json`), times every
pass with GPU timestamps, reads the outputs back as float32 RGBA `.npy`, and writes a result bundle.
File formats: `lab/CONTRACTS.md`.

## Build

```
cd lab/runner
cmake -G Ninja -B build -DCMAKE_BUILD_TYPE=Release     # -DIGL_DIR=... if IGL is not at ~/sources/igl
ninja -C build shaderlab-runner
```

The project `add_subdirectory`s the IGL checkout (`IGL_WITH_VULKAN=ON`, everything else off,
`IGL_DEPLOY_DEPS=OFF`, `IGL_ENFORCE_LOGS=ON`) and builds IGL, IGL's glslang and the runner into
`build/` (gitignored). IGL's own tree is never written to. Toolchain: system gcc 13, C++20, Ninja,
system Vulkan loader/headers 1.3.275. SPIRV-Reflect is compiled from IGL's vendored copy; toml++ and
nlohmann/json are vendored in `third_party/` (see the README there).

## Build (Android, arm64-v8a)

```
lab/runner/build-android.sh        # -> build-android/shaderlab-runner (stripped) + shaderlab-runner.dbg (symbols)
```

Same sources and the same IGL subdirectory, cross-compiled with the pinned NDK r27c
(`build/cmake/android.toolchain.cmake`, `ANDROID_ABI=arm64-v8a`, `ANDROID_PLATFORM=android-26`,
`ANDROID_STL=c++_static` so it runs from `/data/local/tmp` without `libc++_shared.so`). IGL's CMake
forces Vulkan on and OpenGL off for Android; the runner additionally links `log` and `android`
(IGL's Android log handler, AHardwareBuffer support, ATrace macros). Vulkan is loaded by volk at
runtime (`libvulkan.so`); the NDK's headers are 1.3.275, same as the desktop. No window, no
surface, no shell: identical device path to the desktop runner. Pushing and running is done by
`lab android ...` / `lab run --android SERIAL` (`lab/tools/labtool/android.py`, CONTRACTS.md
"Android runner"). Verified on a Pixel 9 Pro XL (Mali-G715, Android 17): selftest cases rgba32f
(max abs error 0), srgb (4.1e-3), ubo (2.4e-4) and g0 (expected failure) all pass.

## Run

```
build/shaderlab-runner --job <dir>/job.json --out <result_dir> [--device N] [--no-validation] [--verbose]
```

- Exit 0 on success, 1 on any error. `result.json` is always written; on failure it has
  `ok=false` and `error` set (and whatever device info was gathered before the failure).
- `--device N` / `SHADERLAB_DEVICE_INDEX=N` picks physical device N in enumeration order
  (the error message for a bad index lists them). Default: the first discrete GPU, else the only
  device when it is an integrated GPU (phones); otherwise an error listing the devices.
- `--info [--device N] [--no-validation]` opens the device like a run and prints
  `{ok, device, devices, notes, state, tools, os}` as JSON on stdout, no job needed (used by
  `lab android devices`).
- `--no-validation` skips `VK_LAYER_KHRONOS_validation`. `--verbose` / `SHADERLAB_VERBOSE=1` also
  forwards IGL's informational logs to stderr.
- Selftest on the real GPU: `selftest/run_selftest.sh` (compiles with the pinned glslang, validates
  with spirv-val, builds job bundles, runs, checks with numpy).

## Design

- **Device.** `igl::vulkan::HWDevice::createContext(config, nullptr, nullptr)` with
  `config.headless=false`, then `HWDevice::create(ctx, desc, 0, 0, ...)`. Width/height 0 means
  `initSwapchain` is skipped, so no `VkSurface` is ever created (NVIDIA 580.178.04 crashes on
  `VK_EXT_headless_surface`, see `lab/DISCOVERY.md` §5). Rendering goes to offscreen textures.
- **Vertex stage.** The contract's fullscreen triangle GLSL, compiled at runtime through IGL's glslang
  path (`ShaderModuleDesc::fromStringInput`; IGL prepends `#version 460`). 3 vertices, no vertex
  buffer, no depth, cull disabled. IGL flips the Vulkan viewport (negative height), so NDC y=+1 is
  texel row 0; with the contract's vertex shader, uv (0,0) lands in texel row 0 = top, and the
  passthrough selftest reproduces the input bit-exactly.
- **Fragment passes.** `ShaderModuleDesc::fromBinaryInput` with the job's SPIR-V. One
  `IRenderPipelineState`, one output `ITexture` (scenario format, size = scenario size x scale,
  `Sampled|Attachment`) and one `IFramebuffer` per pass. Load/store ops from the scenario, clear
  color transparent black.
- **Reflection.** SPIRV-Reflect discovers `sampler2D` bindings (set, binding, name), the uniform
  block (set, binding, size, members with name/offset/size/type) and the I/O locations. Samplers are
  bound by name through the scenario's `samplers` table; uniform members are filled by name into an
  std140 byte image at the reflected offsets (`float`, `int`, `vec2/3/4`, `mat4`; an integer literal
  may fill a float member, nothing else converts). The runner errors out listing: scenario uniforms
  the block does not have, type mismatches, and block members the scenario does not set.
  Requirements checked: exactly one output at location 0, only input location 0, samplers in set 0,
  no push constants, no other descriptor types, bindings below IGL's limits (16).
- **Descriptor sets.** IGL's Vulkan backend hardwires descriptor set 0 to combined image samplers
  and set 1 to buffers (`kBindPoint_Buffers`, `src/igl/vulkan/VulkanContext.h`) and ignores the
  set index written in the SPIR-V when it builds pipeline layouts. CONTRACTS.md declares the uniform
  block at set 0, so the runner rewrites the `DescriptorSet` decoration of every `Uniform` /
  `StorageBuffer` variable to 1 before creating the module (a decoration-only patch: no codegen
  change; reflection runs on the unpatched module). The patch is reported in `notes`.
- **Textures.** Inputs are `.npy` float32 HxWx4 uploaded as `RGBA_F32` sampled textures via
  `ITexture::upload`. Sampler state: linear/linear, no mips, clamp-to-edge; `sampler = "nearest"`
  switches to nearest.
- **Timing.** `IDevice::createTimestampQueries(K*P)` (Vulkan `TimestampQueries`, a
  `VK_QUERY_TYPE_TIMESTAMP` pool with two queries per slot) attached to each render pass through
  `RenderPassDesc::timestampQuery`; the encoder writes the start timestamp before
  `vkCmdBeginRenderPass` and the end after `vkCmdEndRenderPass`, the pool reset is recorded lazily
  before the first render pass of the command buffer. Fidelity is set to `Accurate` (start
  timestamp at `BOTTOM_OF_PIPE`, so consecutive passes do not overlap in the measurement). Each
  sample is one command buffer with K back-to-back executions of the whole chain, slot
  `it*P + p`; the runner reads `getElapsedNanosResult` for every slot after the buffer's fence
  (already converted with `timestampPeriod`; an unavailable slot is an error, never 0) and
  records `sum over K / K` per pass. `warmup` submits are discarded. One query object holds all
  `K*P` slots of a sample, so no submit splitting is needed.
- **Command buffers in flight.** The job's `inflight` (default 3) sets a ring of slots, each with
  its own `ITimestampQueries` and command buffer. `igl::vulkan::TimestampQueries` binds itself to
  the first command buffer that records into it and rejects any other until `reset()`
  (`commandBuffer_` / `resetRecorded_`, and the `vkCmdResetQueryPool` is recorded inside that
  buffer), so one query object (one `VkQueryPool`) per slot is required; results are read with
  `vkGetQueryPoolResults` on that pool alone, unaffected by the other buffers still queued. Submit
  `s` uses slot `s mod inflight`; before the slot is reused the runner calls `waitUntilCompleted`
  on the buffer it holds (submit `s - inflight`; `VulkanImmediateCommands::wait` waits only that
  buffer's fence), reads the GPU clock and the timings, then resets the queries, records and
  submits. `CommandQueue::submit` never blocks the CPU: it `vkQueueSubmit`s with a per-buffer
  fence and a GPU-side wait on the previous submit's semaphore (`lastSubmitSemaphore_`), so
  buffers run strictly in order, back to back, and the GPU never idles between samples while the
  CPU waits. Timing semantics are unchanged from `inflight = 1` (the old submit-then-wait loop);
  the remaining slots are drained in submit order before the readback. IGL's pool has 32
  command buffers (`kMaxCommandBuffers`; `acquire()` stalls when all are busy), so `inflight` is
  capped at 16.
- **Readback.** `IFramebuffer::copyBytesColorAttachment` (Vulkan: `vkCmdCopyImageToBuffer` into
  IGL's staging buffer; it flips the image vertically, which the runner undoes), then the raw texels
  are decoded to float32 RGBA: RGBA8 to [0,1]; RGBA8_SRGB decoded with the sRGB EOTF; RGBA16F
  half->float; R11G11B10F and RGB10A2 unpacked (`RGB10A2` is IGL `RGB10_A2_UNorm_Rev` =
  `VK_FORMAT_A2R10G10B10_UNORM_PACK32`); R16F/R32F fill G,B=0, A=1. Formats outside the contract
  list, or not usable as a sampled attachment on the device, fail with a message naming the format.
- **Validation capture.** IGL routes `VK_EXT_debug_utils` messages through `vulkanDebugCallback`
  into `IGL_LOG_INFO` ("...Validation layer:..."). The runner installs `IGLLogSetHandler` (declared
  in `src/igl/Log.h`), formats every log call, and records messages containing "Validation layer:"
  with severity from IGL's `ERROR:` / `PERFORMANCE:` prefix. Messages are deduplicated on the text
  with hex handles normalized, and reported as `"<count>x [error|warning] <text>"` with total error
  and warning counts. Verified by enabling the layer's best-practices checks
  (`VK_LAYER_SETTINGS_PATH` with `khronos_validation.validate_best_practices = true`): 260 warnings,
  30 unique, all captured. Non-validation IGL warnings/errors go to stderr.
- **Device info.** `VulkanContext::getVkPhysicalDeviceProperties()` and
  `getVkPhysicalDeviceDriverProperties().driverInfo` ("580.178.04"), `timestampPeriod`,
  `uname`. Features are queried directly with `vkGetPhysicalDeviceFeatures2` through IGL's volk
  table (`ctx.vf_`); `VK_KHR_shader_float_controls2` is newer than the system headers, so its
  feature struct is declared locally (sType 1000528000) and only chained when the extension is
  advertised. `enabled_by_igl` reports what IGL actually turned on. `state` is parsed from
  `nvidia-smi --query-gpu=clocks.gr,clocks.mem,pstate,temperature.gpu,clocks_throttle_reasons.active`
  before (`state`) and after (`state_after`) the run; without nvidia-smi everything is "unknown".
  `locked_clocks` is `null`: nvidia-smi does not expose it. On Android (`sysinfo.cpp`,
  `__ANDROID__`) `state` comes from `dumpsys thermalservice` (ThermalStatus + every HAL
  temperature sensor), `dumpsys battery`, and the GPU clock sysfs node (Mali
  `/sys/class/misc/mali0/device/cur_freq` in kHz, kgsl `gpuclk` in Hz, or a GPU devfreq);
  fields in CONTRACTS.md "Android runner".
- **Tools.** `igl_commit` from `git rev-parse` in the IGL checkout at configure time;
  `runner_build` = shaderopt short HEAD @ UTC configure time.

## Limitations (honest list)

- **`glslang -g0` is incompatible with name-based binding.** `-g0` strips all `OpName`, so
  SPIRV-Reflect sees no sampler or member names. The runner refuses such modules with a message
  saying so (selftest case `g0`). The contract line "compiled with `glslang -V -g0`" needs to change
  to `-V` (or strip debug info only after reflection). The selftest compiles with `-V`.
- `shaderFloatControls2` is reported as supported by the device but IGL does not enable it on the
  logical device (`VulkanFeatures` has no struct for it), so SPIR-V using `FPFastMathMode` through
  float_controls2 will fail validation. Upstreamable IGL patch idea: chain
  `VkPhysicalDeviceShaderFloatControls2FeaturesKHR` in `VulkanFeatures`.
- One color output per pass (location 0); MRT is rejected with a clear error.
- Uniform member types are the contract's (float, int, vec2/3/4, mat4); arrays, structs, bool,
  ivec, 16-bit members are rejected.
- `state.locked_clocks` is never asserted; timing runs share the GPU with whatever else is running.
- Android: state is sampled before and after the run, not per sample (the GPU clock alone is
  sampled per recorded sample, `sample_clock_mhz`, right after that sample's fence), so a short
  job can report the idle GPU clock (150 MHz on the Pixel 9 Pro XL) on both sides while DVFS
  ramped during the samples.
- Android: no validation layer (production devices do not expose one to `/data/local/tmp`
  binaries); `lab` passes `--no-validation`.
- IGL's `copyBytesColorAttachment` reads back through a staging buffer with a synchronous wait per
  pass; fine for readback = last only.
