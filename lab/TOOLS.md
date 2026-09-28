# Tool pins (2026-09-23)

All tools below are vendored through IGL's `deploy_deps.py` or built from that vendored source. No system packages were installed. Paths are relative to `~/sources/igl/third-party/deps/src/`.

| Tool | Version | Binary / source |
|---|---|---|
| IGL | `6804e57c74bba676168a30bba20f976567240e1a` | `~/sources/igl` |
| glslang | 15.3.0 | `glslang/build/StandAlone/glslang` |
| SPIRV-Tools (spirv-val, spirv-opt, spirv-dis, spirv-as) | v2025.2 (a62abcb4) | `glslang/build/External/spirv-tools/tools/` (fetched by glslang's `update_glslang_sources.py`) |
| SPIRV-Headers | vulkan-sdk-1.3.268.0-5-gcca08c6 | `SPIRV-Headers/` |
| SPIRV-Cross | vulkan-sdk-1.4.321.0 | `SPIRV-Cross/build/spirv-cross` |
| SPIRV-Reflect | vulkan-sdk-1.4.304.0 | `gfxreconstruct/external/SPIRV-Reflect/spirv_reflect.{h,c}` (header+source, compiled into the lab) |
| volk | 1.4.304 | `volk/` |
| VMA | vendored | `vma/` |
| glslc | system, `/usr/bin/glslc` | convenience only; glslang above is the pinned compiler |
| FLIP | `flip-evaluator` in `~/sources/shaderopt/.venv` | NVlabs FLIP, Python package |
| Python | 3.12.3 via `uv venv` | `.venv/` with numpy, imageio, opencv-headless, OpenEXR, toml |
| Vulkan | 1.4.312 on RTX PRO 6000 Blackwell, driver 580.178.04 | system loader `libvulkan-dev` |

| Android platform-tools (adb) | 1.0.41, latest zip 2026-09-28 | `~/Android/platform-tools/adb` |
| Android NDK | r27c | `~/Android/android-ndk-r27c` |

## Not yet available

- **IGL full build.** Done 2026-09-24 in `~/sources/igl/build` (Vulkan backend, shell, samples, IGLU; OpenGL off) after the user installed the X11 and GL/EGL dev packages. 29 `*_vulkan` shell sessions built. `HelloWorldSession_vulkan` renders windowed on the RTX PRO 6000 (swapchain BGRA_SRGB).
- **IGL shell `--headless` is unusable on this driver.** NVIDIA 580.178.04 segfaults in `vkGetPhysicalDeviceSurfaceCapabilitiesKHR` for a `VK_EXT_headless_surface` surface; reproduced without IGL by `lab/probes/headless_surface_probe.c`. The lab runner therefore creates the IGL device with no window and zero swapchain size (`HWDevice::create` skips the swapchain when width or height is 0) and renders offscreen. Shell sessions are run windowed when needed.
- **Khronos validation layer.** `VK_LAYER_KHRONOS_validation` is not installed, so acceptance gate 2 (no new validation messages) cannot run yet. Ubuntu has `vulkan-validationlayers` 1.3.275; a Vulkan SDK build would be newer. Needs sudo.
- **malioc, RGA, Xcode, renderdoc.** Not installed; needed for proxy stats and iOS.
- **python3-venv system package.** Absent; `uv` is used instead.
