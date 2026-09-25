# Lab contracts

File formats shared by the runner (C++/IGL), the CPU model (Rust `shader-ir`), and the orchestration (Python `lab`). Change any of these only by editing this file first.

## Images: `.npy`

All images cross process boundaries as NumPy `.npy` files, dtype `float32`, shape `[height, width, 4]`, RGBA, row 0 = top of the image, linear values (no sRGB encoding). Every GPU output is read back and stored this way regardless of the GPU texture format; format loss (RGBA8, R11G11B10) is then visible in the data. Python converts `.npy` to EXR and PNG for humans. Integer or mask outputs are still stored as float32.

## Shaders

- Baseline sources: `lab/shaders/<shader>.frag` (GLSL 450, Vulkan semantics), compiled with the pinned glslang: `glslang -V -o <shader>.spv` (never `-g0`: SPIRV-Reflect binds by `OpName`, so debug names must stay). The vertex stage is always the runner's built-in fullscreen triangle, which writes `layout(location = 0) out vec2 uv` with uv in [0,1], (0,0) at the top-left, sampled at pixel centers.
- Fragment shader interface conventions: `layout(location = 0) in vec2 uv`; outputs `layout(location = N) out vec4`; samplers `layout(set = 0, binding = B) uniform sampler2D <name>`; one uniform block `layout(set = 0, binding = B, std140) uniform Params { ... } u` whose members are set by name; members named `pad*` are optional and zero-filled by the runner. Push constants are not used. Bindings are discovered by SPIRV-Reflect, never hardcoded. IGL's Vulkan backend hardwires descriptor set 0 = textures and set 1 = buffers, so the runner rewrites the `DescriptorSet` decoration of uniform-block variables to 1 before pipeline creation (decoration-only, reported in the result's `notes`); shaders keep writing `set = 0`.
- Sampler state: linear min/mag filter, no mipmaps (lod 0), clamp-to-edge, unless a pass sets `sampler = "nearest"`.

## Scenario: `lab/scenarios/<name>.toml`

```toml
[scenario]
name = "bloom_hdr_gradient"
split = "train"            # train | holdout
width = 1920
height = 1080

[[inputs]]                 # source images, produced by `lab gen-inputs` before the run
name = "scene"
generator = "hdr_gradient" # or: file = "relative/path.npy"
seed = 1
params = { peak = 16.0 }

[[passes]]                 # executed in order
name = "threshold"
shader = "bloom_threshold" # baseline: lab/shaders/bloom_threshold.frag; variants override per pass in the job
samplers = { u_scene = "scene" }           # sampler name -> input name or earlier pass output name
uniforms = { threshold = 1.0, knee = 0.5 } # uniform block members by name; vecN as arrays
output = { format = "RGBA16F", scale = 0.5 } # texture format; resolution = scenario size * scale
load = "dont_care"         # dont_care | clear | load
store = "store"            # store | dont_care

[[passes]]
name = "composite"
shader = "bloom_composite"
samplers = { u_scene = "scene", u_bloom = "blur_v" }
uniforms = { intensity = 0.8 }
output = { format = "RGBA8_SRGB", scale = 1.0 }

[quality]
outputs = ["composite"]    # downstream outputs judged by the gates; default: the last pass
```

Accepted `format` values: `RGBA8`, `RGBA8_SRGB`, `RGBA16F`, `RGBA32F`, `R11G11B10F`, `RGB10A2`, `R16F`, `R32F`. Uniform member types: `float`, `int`, `vec2/3/4` (array of numbers), `mat4` (16 numbers, column-major).

## Job bundle: a directory with `job.json`

```json
{
  "schema": 1,
  "scenario": "../../scenarios/bloom_hdr_gradient.toml",
  "variant_id": "baseline",
  "passes": { "threshold": "spv/threshold.spv", "blur_h": "spv/blur_h.spv" },
  "inputs": { "scene": "inputs/scene.npy" },
  "samples": 30,
  "iterations": 8,
  "warmup": 5,
  "readback": "last"
}
```
- `passes` maps every pass name in the scenario to a SPIR-V file (baseline or variant). Paths are relative to the job directory.
- `iterations` = K back-to-back executions of the whole chain inside one command buffer per sample. `samples` = N submits, each giving one timing per pass (already divided by K). `warmup` submits are executed and discarded.
- `readback` = `last` (images from the final sample only) or `none`.

## Result bundle: a directory with `result.json` and images

```json
{
  "schema": 1,
  "variant_id": "baseline",
  "scenario": "bloom_hdr_gradient",
  "ok": true,
  "error": null,
  "device": { "name": "...", "driver": "580.178.04", "api": "1.4.312", "vendor_id": 4318, "device_id": 0, "timestamp_period_ns": 1.0, "os": "Linux 6.17.0-20-generic", "features": { "shaderFloat16": true, "storageBuffer16BitAccess": true, "shaderFloatControls2": true } },
  "tools": { "igl_commit": "6804e57...", "runner_build": "..." },
  "state": { "thermal": "unknown", "clocks_mhz": { "gpu": 2500, "mem": 14000 }, "power_state": "P0", "locked_clocks": false },
  "validation": { "errors": 0, "warnings": 0, "messages": ["..."] },
  "timings_ns": { "threshold": [12345.0, ...], "blur_h": [...] },
  "images": { "threshold": "threshold.npy", "composite": "composite.npy" }
}
```
- `timings_ns[pass]` has `samples` entries, each the GPU time of one execution of that pass (timestamp delta / K), in nanoseconds using `timestamp_period_ns`.
- `validation.messages` holds every Khronos validation message text, deduplicated, with counts. Any error makes gate 2 fail but the run still completes.
- `images[pass]` is written for every pass when `readback = last`.

## Results log: `lab/results.jsonl`

One line per experiment, written by `lab`:
```json
{"ts": "...", "scenario": "...", "split": "train", "variant_id": "...", "edit_ops": [...], "device_fingerprint": "sha256 of device json", "median_ns": {"pass": ...}, "ci95_ns": {"pass": [lo, hi]}, "metrics": {"composite": {"flip_mean": 0.0, "flip_p99": 0.0, "flip_max": 0.0, "abs_max": 0.0}}, "tolerance": {"source": "none|site|shader|budgets|pipeline", "value": {...}}, "gates": {"1": true, "2": true, "3": true, "4": null, "5": null}, "result_dir": "..."}
```

## CPU model (`shader-ir`) CLI

```
shader-ir roundtrip <in.spv> [--out out.spv]      # exit 0 iff body identical; prints header diff
shader-ir eval --spv pass.spv --width W --height H --mode f32|f64|f16 \
    --sampler name=path.npy ... --uniform name=value ... --out out.npy [--discard-value nan]
```
`eval` runs the fragment entry point at every pixel center with the same uv convention as the runner. `--sampler-weight-bits N` (the lab uses 8) models the GPU's fixed-point texture coordinates: the unnormalized coordinate is rounded to nearest with N fractional bits before the bilinear weights are taken; measured to reproduce NVIDIA 580.178.04 exactly. The CPU-vs-GPU tolerance is defined in code units of the storage format in `lab/lift_tolerances.toml`. Discarded pixels are written as NaN in all channels (the runner leaves them at the clear/load value, so comparisons mask NaN pixels). Uniform values use the scenario syntax.

## Variants: `lab/variants/<shader>/<variant-id>/`

Contains `variant.json` (the edit-op list, parent variant, tool versions), the produced `.spv`, and an `annotations.toml` sidecar when the variant depends on site annotations. Originals in `lab/shaders/` are never modified.

## M2: analysis and rewrite CLIs (`shader-ir`)

**Debug builds for source mapping.** `lab build` also compiles `lab/build/spv/<shader>.g.spv` with `glslang -V -g`. Its instruction body, after dropping `OpLine`/`OpNoLine`/`OpString`/`OpSource`/`OpModuleProcessed`, is the same sequence as the measured `-V` build, so the k-th body instruction of the measured build maps to the k-th of the debug build and thereby to a source line. Result IDs differ between the two builds; measured-build IDs are the canonical ones everywhere.

```
shader-ir analyze --spv x.spv [--debug-spv x.g.spv] [--ranges ranges.json] --out analysis.json
```
```json
{"shader": "fxaa", "entry": "main", "bound": 349,
 "instructions": [
   {"id": 57, "op": "OpFMul", "ext": null, "type": "vec3<f32>", "func": "main", "block": 3, "index": 41, "line": 22,
    "name": null, "rate": "pixel", "sinks": ["address"], "operands": [55, 56],
    "range": {"min": 0.0, "max": 63.9, "nan": 0, "inf": 0, "samples": 129600}}
 ],
 "samplers": [{"name": "u_src", "binding": 0, "samples": [{"id": 60, "coord_id": 59, "coord_kind": "uv_exact", "offset": null}]}],
 "outputs": [{"location": 0, "id": 12, "type": "vec4<f32>"}],
 "summary": {"pixel": 120, "uniform": 20, "const": 30, "sink_sites": 14, "float_sites": 130, "candidate_sites": 90}}
```
- `rate`: `const` (derived only from constants), `uniform` (constants and uniform-block loads only), `pixel` (anything touching Location inputs, FragCoord, image ops, derivatives, or phi from divergent control flow).
- `sinks`: `address` (feeds an image sample/fetch coordinate, an `OpAccessChain` index, or an array index), `control` (feeds `OpBranchConditional`/`OpSwitch`/`OpSelect` condition), `discard` (feeds a branch that dominates an `OpKill`), `convert` (feeds `OpConvertFToS/U`). A site with any sink is off-limits for lossy edits. Float-typed instructions without sinks are `candidate_sites`.
- `coord_kind`: `uv_exact` (the coordinate is the Location-0 input itself), `uv_offset` (uv plus a per-dispatch constant/uniform offset; `offset` is the constant when known), `other`.
- `range` comes from `--ranges`, a JSON written by `eval --profile ranges.json [--stride N]`: `{"<id>": {"min","max","nan","inf","samples"}}` over every evaluated pixel (every N-th pixel in each dimension when `--stride N`), for every float-typed result.

```
shader-ir eval ... --f16-sites 57,58,90 --out out.npy      # round the listed float results to f16 (sensitivity / demotion prediction)
shader-ir eval ... --f16-all                               # every float site
```

```
shader-ir rewrite --spv in.spv --out out.spv --passes fold,dce,cse,ident,unroll,divconst,powspec,select --ops ops.json [--max-unroll 16]
```
- Applies the named passes in the given order, repeating `fold,dce,cse,ident` to a fixed point. Untouched instructions keep their result IDs (1:1 map preserved); new instructions take fresh IDs above the old bound. Output is validated in-process with spirv-tools.
- `ops.json`: `[{"pass": "fold", "class": "exact", "target": 57, "replaced_by": 401, "detail": "OpFMul %55 %56 -> OpConstant 0.5"}, ...]`. `class` is `exact` (bit-identical on every input by IEEE semantics) or `ulp` (identical in real arithmetic; bounded rounding difference, e.g. `x / c -> x * (1/c)`, `pow(x, 2) -> x * x`).
- Passes: `fold` constant folding incl. GLSL.std.450 on constants; `dce` unused pure results and unreferenced variables; `cse` identical pure instructions where one dominates the other; `ident` `x*1, x+0, x-0, x/1, -(-x), select(c,x,x)`; `unroll` full unroll of loops with constant trip count `<= max-unroll`; `divconst` `x / c -> x * (1/c)` for non-zero, finite, non-denormal constants (`ulp`); `powspec` `pow(x,2)->x*x`, `pow(x,0.5)->sqrt`, `pow(x,1)->x`, `exp(log(x)*c)` untouched (`ulp`); `select` if/else whose both arms are side-effect free single blocks with no image ops, derivatives or kills -> `OpSelect` (exact).
