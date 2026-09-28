# Plugging your project into shaderopt

The lab needs three things from your project: the fragment shaders, the images they run on, and a description of how the passes chain. Everything else (compilation, analysis, variants, measurement, reports) is generic.

## 1. Point the lab at your shaders

Create `lab/project.toml`:

```toml
[project]
shaders = ["/path/to/your/app/shaders/post"]   # one or more directories of *.frag (GLSL 450, Vulkan semantics)
inputs = "/path/to/exported/frames"             # where scenario `file =` inputs are looked up (png, jpg, exr, npy)
budgets = "lab/budgets.toml"                    # optional: your own budgets file
```

`./lab/lab build` compiles every `*.frag` from `lab/shaders/` and the listed directories with the pinned glslang, validates them, and writes `lab/build/spv/<name>.spv` plus a debug build for source-line mapping. Shader names must be unique across directories. Originals are never modified; variants go to `lab/variants/<shader>/<id>/`.

Interface conventions the runner and the CPU model rely on (`lab/CONTRACTS.md`):
- `layout(location = 0) in vec2 uv;` from a fullscreen triangle, (0,0) top-left, sampled at pixel centers. The vertex stage is always the runner's built-in fullscreen triangle.
- Outputs: `layout(location = 0) out vec4`. One color output per pass today.
- Samplers: `layout(set = 0, binding = B) uniform sampler2D name;`. Linear filtering, clamp to edge, no mipmaps unless the pass says `sampler = "nearest"`.
- One uniform block, `layout(set = 0, binding = B, std140) uniform Params { ... } u;`, members set by name from the scenario. `pad*` members are optional. IGL puts buffers in descriptor set 1 internally; keep writing `set = 0`, the runner rewrites the decoration.
- No push constants, no storage buffers, no compute; fullscreen fragment passes only.

If your app's shaders use a different interface (a different vertex layout, gl_FragCoord only, extra varyings), the smallest adaptation is a thin wrapper `.frag` that maps `uv` onto what the shader expects; the wrapper is the shader the lab optimizes.

## 2. Export inputs

Export real frames from your app as PNG (8-bit, sRGB assumed) or EXR (linear float). Put them under the `inputs` directory and reference them from scenarios with `file = "scene_042.exr"` and the texture `format` your app really uses. The lab resamples to the scenario resolution and quantizes to the declared format so the CPU model and the GPU see identical data. Keep a train and a holdout set: everything is judged on both, and the holdout scenarios are what catch overflows and edge cases (they did here, twice).

Synthetic generators (`gradient`, `hdr_gradient`, `edges`, `hdr_edges`, `checker`, `noise`, `nan_inf`, `gbuffer`) are still worth including as extra scenarios; gradients catch banding, `nan_inf` catches f16 overflow.

## 3. Describe the pass chains

One TOML per scenario in `lab/scenarios/` (schema in `lab/CONTRACTS.md`):

```toml
[scenario]
name = "bloom_kitchen"
split = "train"          # or holdout
width = 1920
height = 1080

[[inputs]]
name = "scene"
file = "kitchen_042.exr"
format = "RGBA16F"       # what the app really binds

[[passes]]
name = "threshold"
shader = "bloom_threshold"
samplers = { u_scene = "scene" }
uniforms = { threshold = 1.0, knee = 0.5, exposure = 1.0 }
output = { format = "RGBA16F", scale = 0.5 }

[[passes]]
name = "composite"
shader = "bloom_composite"
samplers = { u_scene = "scene", u_bloom = "threshold" }
uniforms = { intensity = 0.8, exposure = 1.0 }
output = { format = "RGBA8_SRGB", scale = 1.0 }

[quality]
outputs = ["composite"]  # the outputs the gates judge (default: the last pass)
```

Mirror your app's intermediate formats, resolutions and load/store ops: on tilers they dominate the cost of a fullscreen pass, and the format and resolution experiments (`lab graph`) are derived from these values.

## 4. Set the quality budget

`lab/budgets.toml` (or your own file named in `project.toml`):

```toml
[pipeline]
allow_lossy = true     # false: only exact rewrites and within-noise variants are ever accepted
min_speedup = 0.10     # a lossy variant must buy at least this much to spend any quality

[defaults]
color = { metric = "flip",     mean_max = 0.01, p99_max = 0.05 }   # display-encoded outputs
hdr   = { metric = "flip_hdr", mean_max = 0.01, p99_max = 0.05 }   # linear HDR outputs
mask  = { metric = "exact" }

[kinds]                 # which kind each pass output is (unlisted: 8-bit formats are color, else hdr)
composite = "color"

[targets.composite]     # per-output overrides, optionally per platform
p99_max = 0.07
```

Precedence is site annotation, shader annotation, this file, then the pipeline default; annotations in the shader are planned, not required. `lab/lift_tolerances.toml` holds the CPU-model-versus-GPU tolerances in storage code units, with per-shader entries for precision-fragile shaders.

## 5. Add hypotheses (the AI step)

Semantic rewrites are proposed as full replacement sources with a claim: `lab/hypotheses/<shader>/<id>/<shader>.frag` plus `hypothesis.toml` (claim, class, predicted cost, predicted error, the scenarios most likely to expose it). Optional `uniforms.py` computes extra uniform members on the CPU from the scenario's uniforms; optional `inputs.py` provides extra sampler images (LUTs). `./lab/lab hypo` predicts each in the CPU model, measures it, gates it, and records the outcome; falsified hypotheses stay in the log on purpose.

Feed the classification report (`lab/reports/<shader>.classification.md`) to whoever, or whatever, proposes hypotheses: it lists rates, hard sinks with source lines, ranges, per-site f16 sensitivity, sampler coordinate kinds and the fusable pass pairs.

## 6. Devices

Desktop: any Vulkan 1.3 GPU. Lock clocks if you can (`nvidia-smi -lgc`), measure the A/A floor per session (`lab aa`).

Android: install platform-tools and a udev rule for the vendor id, enable USB debugging, then

```bash
./lab/lab android devices      # model, GPU, driver, Vulkan features, thermal state
./lab/lab android push         # the arm64 runner (build with lab/runner/build-android.sh, NDK r27)
./lab/lab aa bloom_kitchen --android SERIAL --inflight 3 --iterations 32 --warmup 20
```

Mobile governors need sustained load: keep three submits in flight, 32 or more executions per submit, and a long warmup. The runner records the GPU clock per sample where sysfs allows it; the statistics keep steady-clock, plateau samples and use round-paired estimates. Thermal status above SEVERE pauses the run. Finalists should be confirmed in the foreground app; the headless path measures the shader, not the app's frame.

iOS: designed (IGL Metal backend, SPIRV-Cross to MSL, jobs via devicectl) and not yet built.

## 7. Run the whole thing

```bash
./lab/lab build && ./lab/lab gen-inputs
./lab/lab aa <a train scenario>          # per device, per session
./lab/lab baseline
./lab/lab lift-check --split train       # CPU model vs your GPU, must pass before trusting predictions
./lab/lab classify
./lab/lab canon                           # exact rewrites (expect no effect on desktop drivers)
./lab/lab hypo
./lab/lab demote
./lab/lab hoist && ./lab/lab approx
./lab/lab graph --split train
./lab/lab search
./lab/lab report
```

Every measurement appends to `lab/results.jsonl` with the device fingerprint, tool versions and thermal/clock state. The per-shader report shows the Pareto frontier per device, the accepted variants with their edit ops, and the rejected ideas with reasons. Accepted variants are SPIR-V (or GLSL for hypotheses) under `lab/variants/`; carrying one into your app is your decision, and the report's evidence is what you carry with it.

## 8. What the lab will not do

- Change your originals or your app.
- Accept anything without a device measurement; CPU numbers are always labeled predicted.
- Touch hard-sink sites (UVs, texture coordinates, branch conditions, discard, float-to-int) with lossy edits unless you endorse a site explicitly.
- Optimize compute or vertex shaders, multiple render targets, or storage buffers (not yet).
