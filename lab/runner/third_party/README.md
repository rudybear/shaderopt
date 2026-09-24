# Vendored single-header dependencies

Downloaded once from the tagged GitHub releases, unmodified. No other network dependencies.

| File | Project | Version | Source | SHA-256 |
|---|---|---|---|---|
| `toml.hpp` | toml++ (marzer/tomlplusplus), MIT | v3.4.0 | `https://raw.githubusercontent.com/marzer/tomlplusplus/v3.4.0/toml.hpp` | `6b5172ad4dd6519aec67b919181fa7a38a2234131e5b2afa232dfe444819783e` |
| `json.hpp` | nlohmann/json, MIT | v3.12.0 | `https://raw.githubusercontent.com/nlohmann/json/v3.12.0/single_include/nlohmann/json.hpp` | `aaf127c04cb31c406e5b04a63f1ae89369fccde6d8fa7cdda1ed4f32dfc5de63` |

`nlohmann_shim/nlohmann/json.hpp` only forwards to `../../json.hpp` so sources can use the
conventional `#include <nlohmann/json.hpp>`.

SPIRV-Reflect is not copied here: `spirv_reflect.c/h` are compiled straight from IGL's vendored
copy (`~/sources/igl/third-party/deps/src/gfxreconstruct/external/SPIRV-Reflect`, vulkan-sdk-1.4.304.0,
see `lab/TOOLS.md`).
