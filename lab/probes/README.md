# Probes

Small standalone programs that isolate a platform behavior from the lab code.

## headless_surface_probe.c

Creates a Vulkan instance with `VK_EXT_headless_surface`, creates a headless surface, and calls `vkGetPhysicalDeviceSurfaceCapabilitiesKHR` on each physical device.

Result on 2026-09-24, NVIDIA RTX PRO 6000 Blackwell, driver 580.178.04, Vulkan 1.4.312:
- NVIDIA device: **segfault inside `libnvidia-glcore.so.580.178.04`** on the capabilities call. Same with `DISPLAY` unset and with all implicit layers disabled.
- llvmpipe: succeeds (minImages=4, maxExtent=16384x16384, 2 formats).

Consequence: IGL's shell `--headless` mode, which uses a headless surface plus a swapchain, crashes on this driver. This is a driver bug, not an IGL bug. The lab's desktop runner creates the IGL Vulkan device with no window and zero swapchain size, so no surface is ever created, and renders to offscreen textures.

Build and run:
```
cc -O1 -o headless_surface_probe headless_surface_probe.c -lvulkan && ./headless_surface_probe
```

## rspirv_roundtrip

Loads a SPIR-V module with `rspirv::dr::load_words` (rspirv 0.12, the same version axiom-compute pins) and immediately re-assembles it, then compares words. `tonemap.frag` is a fragment shader with a sampler, a uniform block, a loop, `mix`/`clamp`/`pow`/`dot`, a helper function and `discard`.

Result on 2026-09-24 with glslang 15.3.0 output (`-V`, `-V -Os`) and after `spirv-opt -O`:
- Body after the 5-word header: **identical** in all three cases.
- Header: only word 2 (generator magic) differs, glslang `0x8000b` → rspirv `0xf0000`. Magic, version, bound and schema are preserved.

Consequence: rspirv's `dr::Module` already satisfies the M1 "faithful lift" gate as the lab's IR substrate, with the one header difference explained. Build and run:
```
cd lab/probes/rspirv_roundtrip && cargo run --release -- path/to/shader.spv
```
