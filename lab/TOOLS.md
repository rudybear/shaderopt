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

## Not yet available

- **IGL library build.** Blocked: IGL's Vulkan CMake hardcodes `VK_USE_PLATFORM_XLIB_KHR`, and `X11/Xlib.h` is not installed. The GLFW shell additionally needs xrandr, xinerama, xcursor and xi headers. Needs `sudo apt install libx11-dev libxrandr-dev libxinerama-dev libxcursor-dev libxi-dev`.
- **malioc, RGA, adb, NDK, Xcode, renderdoc.** Not installed; needed only when mobile devices arrive or for proxy stats.
- **python3-venv system package.** Absent; `uv` is used instead.
