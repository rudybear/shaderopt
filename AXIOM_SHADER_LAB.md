# AXIOM Shader Lab: Agent Brief

Version 4 (2026-09-23). Supersedes v3. Changes from v3: the mission is an automatic shader optimizer, not a precision-demotion gate; three proposal engines (exact static, AI semantic, lossy); quality tolerance is an optional pipeline-wide parameter; inputs filled in for this machine; axiom-compute is read-only and guarded by the `axiom-guru` skill.

Read it fully before starting. Fields still marked `{{…}}` are unknown and must be confirmed in M0.

## 0. Inputs

- **Lab workspace:** `~/sources/shaderopt` (this directory). All lab code, scenarios, variants, results and reports live here.
- **Shader project:** none yet. The user will provide shader samples along the way. Until then, use IGL's render sessions under `~/sources/igl/shell/renderSessions/` and `shell/renderSessions/shaders/` as the starter corpus. Treat every supplied shader as a fullscreen post-processing pass unless told otherwise.
- **Build:** `{{BUILD_CMD}}` for supplied shaders. For the starter corpus, compile with `glslc` (present at `/usr/bin/glslc`) or glslang once installed.
- **Pass chains:** infer in M0.
- **AXIOM workspace:** `~/sources/axiom-compute` (READ-ONLY, see §5). Crates: `axc-lexer`, `axc-parser`, `axc-hir`, `axc-optimize`, `axc-codegen` (rspirv Vulkan-flavor path), `axc-runtime` (ash dispatch), `axc-driver` (CLI, MCP server, `axc optimize` autotuner), `axc-py`. Design doc: `DESIGN.md`. Project rules: `CLAUDE.md`, `AGENTS.md`. Existing annotations of interest: `@strategy { ?holes }`, `@equiv_fp_tol`, `@strict`, `@intent`. Note that axiom-compute's own docs call it a compute language, "NOT a shader/graphics language"; see §2.1.
- **AXIOM guardian:** the `axiom-guru` skill (`~/.claude/skills/axiom-guru/`) and the gatekeeper protocol at `~/sources/claude-memory/projects/_shared/gatekeeper_protocol.md`. Both are read-only.
- **Harness base:** IGL at `~/sources/igl`, pinned to commit `6804e57c74bba676168a30bba20f976567240e1a` (2026-09-23, "Use a `const` designated initializer in `ParametricVertexData`"). Cloned blobless; run `deploy_deps.py` in M0. IGL already provides `ITimestampQueries` (`src/igl/TimestampQueries.h`, `CommandBuffer.h`) and `shaderFloat16` / 16-bit-storage feature structs (`src/igl/vulkan/VulkanFeatures.cpp`).
- **Devices:**
  - Desktop: NVIDIA RTX PRO 6000 Blackwell Workstation Edition, driver 580.178.04, Vulkan 1.4.312. Lavapipe (llvmpipe) is also present for CPU-side sanity runs, never for timing.
  - macOS, optional: `{{Apple Silicon Mac, if available}}`
  - Android: `{{models and GPUs}}` (no adb or NDK installed yet)
  - iOS: `{{models}}` (no Xcode on this Linux host; iOS work needs a Mac)
- **Apple signing:** `{{team ID / provisioning}}`
- **Real input images:** `{{INPUTS_DIR}}/train/` and `{{INPUTS_DIR}}/holdout/`. Until provided, the synthetic generators in §3 are the only inputs.
- **Debug tools:** renderdoc-skill `{{path}}` and metal-ai-skill `{{path}}`. Neither found on this machine yet.
- **Static proxies:** `{{malioc core, RGA target}}`. Neither installed yet.
- **Tooling status on this host (2026-09-23):** IGL's `deploy_deps.py` vendors nearly everything. Built and pinned in `lab/TOOLS.md`: glslang 15.3.0, SPIRV-Tools v2025.2 (spirv-val, spirv-opt, spirv-dis, spirv-as), SPIRV-Cross vulkan-sdk-1.4.321.0, SPIRV-Reflect vulkan-sdk-1.4.304.0 (header+source), volk, VMA, and NVlabs FLIP in the lab venv (`.venv`, created with `uv`). Not yet: the IGL library itself, blocked on the X11 dev headers (see `lab/TOOLS.md`); malioc, RGA, adb, NDK, renderdoc, Xcode. Use the pinned binaries, never the system `glslc`, for anything that produces a measured artifact.

## 1. Mission

Build a measurement-driven lab that **automatically finds the most performant version of each post-processing shader** on desktop, Android and iOS, within a quality budget that the user controls.

The lab is a compiler-plus-experiments loop, not a single transform:

1. **Exact static optimization.** Compiler and linker-time rewrites that preserve semantics: constant folding and premultiplication, algebraic simplification and reassociation, common-subexpression and dead-code elimination, strength reduction, loop-invariant and uniform-rate hoisting, branch flattening or specialization, FMA and vector-lane packing. These canonicalize the shader and shrink the search space. Drivers also do some of these, so each is still measured.
2. **AI-driven semantic analysis.** The differentiator. Drivers never change program logic; the lab may. An LLM step reads each shader as an algorithm and proposes rewrites a compiler cannot: fewer or bilinear-assisted filter taps, refit tone curves and color transforms as cheaper polynomials or LUTs, dropping branch arms whose outputs are visually equivalent, computing near-uniform per-pixel work once per frame, approximations invisible after the downstream pass, removing redundant reads and passes. Every proposal is a typed edit-op with a predicted cost and a predicted error, and every proposal must be verified by experiment. No proposal is accepted on the model's word.
3. **Lossy transforms with explicit budgets.** Precision demotion, range-profiled approximations, cheaper intermediate formats, reduced resolution, pass fusion, fast-math flags.
4. **Experiments on real devices decide.** A cross-platform playback sandbox built on IGL runs every variant identically on real hardware. It produces per-device GPU time and image error. Static estimates and CPU-model predictions only rank candidates; they never accept one.

For each shader and device class, deliver a Pareto frontier of GPU time vs. image error, plus accepted variants backed by evidence.

**Quality tolerance is an optional, pipeline-wide parameter.** The user may allow a quality regression up to a stated threshold when it buys a significant speedup. When no tolerance is given, only exact rewrites and variants within the A/A noise floor of the baseline are accepted. When a tolerance is given, the Pareto frontier is still reported in full; the tolerance only decides which points are marked accepted, so changing it never requires rerunning experiments. Precedence, most specific wins: a site-level annotation in the shader, then a shader- or pass-level annotation, then `lab/budgets.toml`, then the pipeline default. Shader annotations are allowed but never required. See §7 and §9.

API legality is covered by `spirv-val` and the validation layers. The problems the lab solves are (a) finding rewrites, exact and semantic, that a driver will not find, and (b) an honest acceptance gate for anything lossy.

## 2. Architecture

The core design is "AXIOM is the lab, SPIR-V is the artifact":

```
GLSL ─glslang─► SPIR-V (artifact) ─lift─► AXIOM MIR (1:1 map to SPIR-V result IDs)
                                            │ exact static passes (fold · CSE · strength-reduce · hoist · flatten)
                                            │ analyses: rate · range · sensitivity · sinks · pass graph
                                            │ AI semantic proposals (typed edit-ops, predicted cost/error)
                                            │ lossy edits (holes / strategies / demotion / approximation)
                                            │ CPU numerics over scenario images: f64 ref · f32 · f16 emu
                                            ▼
                  patch edits into SPIR-V (rspirv) ─► spirv-val
                     ├─ Vulkan: SPIR-V as is ──────────────► desktop runner · Android runner
                     └─ Metal: SPIRV-Cross ─► MSL ─► xcrun metal check ─► iOS runner · macOS runner
                                            ▼
          result bundles (images · per-pass timings · device state) ─► gates per device ─► Pareto ─► reports
```

- **Every transform is a typed edit-op over AXIOM MIR**, whether exact, semantic or lossy. Exact ops carry zero predicted error. The M5 search composes all kinds under Pareto selection.
- **No new backends yet.** Don't build an AXIOM→SPIR-V graphics backend or an AXIOM GLSL frontend unless approved after M3.
- **Reuse AXIOM's constructs.** Map lab concepts onto axiom-compute's existing constructs: `@strategy { ?holes }` for enumerable choices, `@equiv_fp_tol` for tolerance, the `axc optimize` grid search and the `axc mcp` bridge for LLM-driven exploration. Verify each against the current `DESIGN.md` before relying on it.
- **Shader profile needed in AXIOM:**
  - vector/matrix types, GLSL.std.450 ops, structured control flow, fragment inputs/outputs, samplers and images
  - annotations: `@rate(const|uniform|pixel)`, `@range`, `@tolerance`, `@sink(address|control|discard)`
  - CPU emulation of the opaque ops: texture sampling (`texelFetch` exact; bilinear and trilinear approximate, with the approximation documented) and derivatives over 2×2 quads
- **Codegen fallback.** If AXIOM's native path isn't ready for the shader profile, write a MIR interpreter for it inside the lab.

### 2.1 axiom-compute is compute-first; expect rendering gaps

axiom-compute's prebaked optimization pipeline has shown results on compute shaders. It has no SPIR-V lift, no fragment-stage profile, no sampler or derivative model, and its docs explicitly scope it to compute. Rendering support will need changes. Handle them as follows:

- The lab never edits `~/sources/axiom-compute`. Not a line.
- When the lab hits an AXIOM limitation, write `FEATURE_REQUEST.md` in the lab root in the gatekeeper format (what, why, impact, minimal reproduction, proposed fix), mark the task BLOCKED in `lab/NOTEBOOK.md`, and invoke the `axiom-guru` skill with that file.
- The skill produces either a workaround or a change proposal. Proposals require the user's approval before anything is implemented, and the implementation happens in the axiom-compute repo through its own pipeline, not from the lab.
- Until a proposal lands, the lab uses the workaround or the lab-local fallback (interpreter, lab-side pass), and records the dependency in `lab/DISCOVERY.md`.
- Expected first requests, to be confirmed in M0: SPIR-V→MIR lift, fragment I/O and sampler types, GLSL.std.450 coverage, the four shader annotations above, and an edit-op patching API that preserves result IDs.

## 3. Playback sandbox

The unit of playback is a **scenario** (TOML), which specifies:
- the pass chain and each pass's shader
- inputs: image files, or synthetic generators with seeds
- uniforms: fixed, or swept over ranges
- intermediate formats and resolutions
- load/store ops
- the quality tolerance override for this scenario, if any

Mirror the app's formats, resolutions and load/store ops. On tilers they dominate the cost of a fullscreen pass.

**Harness rules:**
- **Built on IGL.** Implement the sandbox as one IGL render session hosted by IGL's shell apps on desktop, Android and iOS. The Vulkan backend takes the SPIR-V variants directly, and the Metal backend takes MSL from SPIRV-Cross.
- **IGL changes.** If the lab needs something IGL lacks, write it as a small, separate patch series that could be upstreamed. Keep lab-only logic out of IGL. Start from what IGL already has: `ITimestampQueries` and the Vulkan `shaderFloat16` / 16-bit-storage feature structs.
- **Bindings.** Derive bindings from SPIR-V reflection (SPIRV-Reflect or SPIRV-Cross), so adding a shader only needs a scenario.
- **Vertex stage.** Use the project's vertex shader, or a standard fullscreen triangle. Keep it identical across variants.
- **Synthetic generators:**
  - gradients (they catch banding)
  - HDR and extreme luminance
  - high-frequency patterns and edges
  - noise
  - NaN/Inf/negative/denormal inputs
  - several resolutions and aspect ratios (they catch UV precision loss)

  The train/holdout split applies to scenarios as well as images.
- **Outputs.** Write raw float (EXR) for metrics and PNG for viewing.
- **Timing, per pass:**
  - Use IGL's timestamp queries where they cover these cases; otherwise reach the native command buffers through IGL's backend-specific interfaces.
  - Vulkan: timestamps outside render passes, never inside them on tilers.
  - Metal: command-buffer GPU start/end times, or stage-boundary counter sampling where supported.
  - Optionally run K back-to-back executions per sample to rise above timer resolution.
- **Device state per sample.** Record thermal state, clocks where readable, battery/charging state, and OS, driver and GPU identifiers.
- **Data-driven runners.** Variants arrive as job bundles (SPIR-V or MSL source, scenario, inputs), never as rebuilt apps. Results return as result bundles, cached by content hash.

**Runners:**
- **Desktop:** a local process on IGL's Vulkan backend. This is the only runner available today and is where M0 through M3 happen.
- **Android:**
  - For fast iteration, a headless native binary on IGL's Vulkan backend, run via `adb` from `/data/local/tmp`.
  - For confirming finalists, the IGL shell APK in the foreground. Governor and DVFS behavior can differ for shell processes.
- **iOS:**
  - The IGL shell app (or an XCTest bundle) hosting the sandbox, installed once.
  - Jobs go in and results come out via `xcrun devicectl` or `xcodebuild test`; verify the flags on the installed Xcode.
  - MSL compiles at runtime, so a new variant never needs a rebuild.
- **macOS Metal:** IGL's Metal backend, for fast Metal iteration only. iOS devices remain the ground truth.

## 4. Milestones

Milestones marked **[STOP]** end the same way: write the report, commit, fire the notification hook if configured, and wait for review.

### M0: Discovery [STOP]

- **Shaders and pass chains.** Inventory the starter corpus (IGL render sessions) and any supplied shaders. Who reads which output, at what format and resolution.
- **AXIOM's real state.** Which crates build, what HIR and MIR support, which of the §2 shader-profile features exist, test status. Produce the first `FEATURE_REQUEST.md` drafts for the gaps and run them through `axiom-guru`.
- **Tooling.** Already pinned in `lab/TOOLS.md`. Verify each binary runs and add a `lab tools` check that fails on any version drift. Android and Apple tooling only when devices exist.
- **Devices.** Desktop features: `shaderFloat16`, 16-bit storage and `VK_KHR_shader_float_controls2`. Mobile devices are pending.
- **IGL.** Deps are fetched. Build the pinned commit for desktop once the X11 headers are installed, then run one existing session and the GPU timer session. Confirm the timestamp path and how `shaderFloat16` is enabled at device creation.

Deliver `lab/DISCOVERY.md` with the plan and the gaps.

### M1: Sandbox MVP and faithful lift [STOP]

1. **Baseline.** One pass chain plays with unmodified shaders on desktop (and on one Android and one iOS device once available). Record baseline outputs and timings, and the A/A noise floor per device.
2. **Cross-device baseline diff.** Flag shaders whose baselines already disagree beyond tolerance across devices. These are precision-fragile.
3. **Faithful lift.**
   - An empty edit set must reproduce the SPIR-V byte for byte. Only header fields may differ, and any difference must be explained.
   - The AXIOM f32 CPU output must match the desktop GPU output within a documented tolerance.

Nothing else proceeds until M1 holds.

### M2: Classification and exact canonicalization

- **Rate.** Classify each value as constant, uniform (per dispatch) or per-pixel.
- **Hard sinks.** These get zero budget:
  - UVs and sample offsets (fp16 UVs lose texel precision at high resolutions)
  - texture coordinates and indices
  - branch conditions
  - `discard`
  - float→int conversions
- **Ranges.** Run the CPU model over scenario pixels, using full images or stratified subsets. Cross-check on devices with instrumented variants that write intermediates to extra outputs.
- **Soft sensitivity.** Inject perturbations at each candidate site in the CPU model, sized to the precision loss under consideration.
- **Pass graph.** Mark adjacent passes as fusable when the consumer reads the producer only at its own pixel.
- **Exact static passes.** Implement the §1 item-1 rewrites as edit-ops. Each must be provably semantics-preserving or, for float reassociation, within a documented ULP bound in the f64 model. Run them on every shader, measure each on the desktop runner, and log which ones the driver already performs (zero measured gain) so the search deprioritizes them. This log applies only to exact rewrites; semantic rewrites are never pruned on this basis.

Deliver `lab/reports/<shader>.classification.md` with source-line mapping, and `lab/reports/<shader>.canonical.md` with the exact rewrites applied and their measured effect.

### M2.5: AI semantic proposals [STOP after the first shader's proposal set is reviewed]

- Give the model the canonical MIR, the source, the classification report, the range profiles and the scenario images.
- It returns a set of semantic rewrite hypotheses, each as a typed edit-op with: the algorithmic claim, the predicted cost change, the predicted error and where it shows up, and the scenarios most likely to expose it.
- Every hypothesis runs through the CPU model first, then the desktop runner. Log accepted and rejected hypotheses with reasons in `lab/NOTEBOOK.md`.
- Hard-sink sites stay off-limits here too.

### M3: Precision demotion across platforms [STOP after the first shader is accepted or conclusively rejected]

1. Demote one site at a time.
2. Combine sites greedily by benefit per unit of error.
3. Accept per platform.

Platform notes:
- Desktop drivers often ignore `RelaxedPrecision`.
- Mobile Vulkan drivers usually honor it.
- On Metal, only explicit float16 is reliable; SPIRV-Cross emits `half`.

Test both forms where they differ.

### M4: More transforms

- **Range-profiled approximations.** Replace `pow`/`exp`/`log`/trig with minimax polynomials over the observed operand ranges, each with a stated error bound.
- **Hoisting to the CPU.** Compute uniform-only subexpressions on the host and pass them in as new uniforms. The harness owns uniforms, so this is directly testable.
- **LUTs.** 1D as const arrays and 2D as textures. Dynamically indexed arrays can be slower than the math they replace, so measure.
- **Intermediate formats.** For example RGBA16F → R11G11B10, RGB10A2 or RGBA8.
- **Resolution scaling.** Run intermediate passes at half or quarter resolution and upsample.
- **Pass fusion.** Fuse the fusable pairs from M2. On tilers, each fused pair saves a full-resolution write and read.
- **Fast-math flags.** Use `FPFastMathMode` where float_controls2 is supported.

Graph-level wins (formats, resolution, fusion, hoisting) become proposals for the app. Nothing in the app changes without the user's approval.

### M5: Search (only after review)

- The model proposes typed edit-ops over AXIOM MIR, drawing on all of M2, M2.5, M3 and M4.
- Evolutionary search combines them under Pareto selection (time vs. error) per device class. Exact ops cost nothing in error, so the search prefers them wherever they help.
- Finish with the final report.

## 5. Rules

- **Originals.** Never modify original shaders or app code. Variants go in `lab/variants/<shader>/<variant-id>/`.
- **axiom-compute is read-only.** Never modify `~/sources/axiom-compute`. Every AXIOM concern goes through `FEATURE_REQUEST.md` and the `axiom-guru` skill, per §2.1. Log each request and its outcome in `lab/AXIOM_REQUESTS.md`.
- **IGL is upstream code.** Lab changes to `~/sources/igl` live on a branch as separate, upstreamable commits and are listed in `lab/IGL_PATCHES.md`.
- **Honest measurement.** Never fabricate or simulate a measurement, or present an estimate as measured. If a tool or device is unavailable, stop and report. Static stats (malioc, RGA) and CPU-model predictions are labeled "proxy" or "predicted".
- **AI proposals are hypotheses.** A model's claim that a rewrite is equivalent or cheaper is never evidence. Only the CPU model and device runs are.
- **Hard sinks.** Hard-sink sites are off-limits unless the user explicitly endorses a specific site. Log each endorsement.
- **Tolerance is opt-in.** No lossy variant is marked accepted unless a tolerance applies to it through the §1 precedence chain. Record the effective tolerance and its source with every accepted result.
- **Reproducibility.** One command per stage. Each result records tool versions, the IGL commit, the SPIRV-Cross version, the device fingerprint and thermal state.
- **Scheduling.** Run one job at a time per device; different devices may run in parallel. Parallelize analysis per shader if the setup supports sub-agents.
- **Device care.** Cool down between batches, and pause a device whose thermal state stays elevated.
- **Negative results.** Log rejected experiments with reasons.
- **Tests.** Before every commit, run the round-trip tests, the analysis golden tests on tiny hand-written shaders, the exact-pass equivalence tests, and a `lab verify` smoke run.

## 6. Measurement protocol

- **Desktop.** Lock clocks with `nvidia-smi -lgc <min>,<max>` (reset with `-rgc`), warm up, then take N ≥ 30 samples.
- **Mobile.** Clocks can't be locked, so design around drift:
  - Interleave baseline and variant in randomized order within each batch.
  - Drop samples taken under thermal throttling. Read thermal status via `PowerManager` or `dumpsys thermalservice` on Android, and `ProcessInfo.thermalState` on iOS.
  - Run the A/A noise floor per device, per session.
  - In the Android APK, use sustained performance mode where supported. On iOS, keep the app in the foreground with the idle timer disabled.
- **Statistics.** Report the median with a 95% bootstrap CI. Accept only if the improvement exceeds max(2%, noise floor) and the CI excludes zero. Evaluate per device.

## 7. Acceptance gates

A variant is judged separately on each device class and must pass all five:

1. `spirv-val` passes, SPIRV-Cross translation and the MSL compile succeed for Apple targets, and the variant uses only features the device supports.
2. No new validation messages, from the Vulkan layers or from Metal API and shader validation.
3. Quality is within the effective tolerance on **both train and holdout** scenarios, on **every downstream output** of the chain, compared against the same device's baseline. With no tolerance in effect, "within" means within the A/A noise floor of the baseline.
4. The timing gate from §6 passes. When a tolerance is being spent, the speedup must also meet the `min_speedup` that accompanies it (§9).
5. On mobile, finalists are confirmed in foreground-app mode. Confirmation in the real app needs the user's approval.

A variant can win on one platform and lose on another. Reports are per device class, and the end state may be platform-specific shader variants.

## 8. Quality and numerics

- **References.** Compare each variant against the same device's baseline. The CPU f64 model is the absolute reference that tells you which device drifts.
- **Reporting.** Always report p99 and max, not just the mean. Gradient scenarios exist to catch banding that means hide.
- **f16 emulation.** Computing in f32 and rounding each op to f16 is exact for basic IEEE ops, but not for GPU transcendentals or denormal flushing. Treat it as a prediction; the device decides.
- **Semantic rewrites.** Compare against the baseline output, not against the rewritten algorithm's own reference. The question is always "does the frame still look right", never "does the new formula compute what it says".

## 9. Budgets template: `lab/budgets.toml`

The values below are placeholders; tune them per project. Everything here is optional. An absent section means no tolerance and only exact or within-noise variants are accepted.

```toml
[pipeline]
# Global opt-in. When false, [defaults] and [targets] are ignored.
allow_lossy = false
# A lossy variant must buy at least this speedup to spend any quality budget.
min_speedup = 0.10

[defaults]
color = { metric = "flip",     mean_max = 0.01, p99_max = 0.05 }
hdr   = { metric = "flip_hdr", mean_max = 0.01, p99_max = 0.05 }
mask  = { metric = "exact" }

[targets."{{pass or output name}}"]
# per-output overrides; optional per-platform overrides:
# platforms.android = { p99_max = 0.07 }
```

Shader-level overrides use the same fields via annotation, for example `@tolerance(metric="flip", p99_max=0.03)` on a pass or output, or `@tolerance(ulp=4)` on a site. Site beats pass, pass beats this file, this file beats the pipeline default.

## 10. Outputs and conventions

- `lab/results.jsonl` holds one record per experiment, including the device fingerprint, the edit-op list, and the effective tolerance with its source.
- `lab/reports/<shader>.md` holds per-device Pareto frontiers, accepted variants with diffs and evidence, and rejected ideas with reasons.
- `lab/NOTEBOOK.md` is an append-only log of hypotheses and outcomes, including every AI semantic proposal.
- `lab/AXIOM_REQUESTS.md` and `lab/IGL_PATCHES.md` track upstream dependencies.
- `lab/TOOLS.md` pins every tool version.
- Follow the user's CLAUDE.md / AUTONOMOUS.md / VERIFY.md conventions if they exist; otherwise create them in the lab root. VERIFY.md lists the commands that re-check every accepted result on every device.
- End each session with a handoff note: state, next step, open questions.
- Use renderdoc-skill and metal-ai-skill to debug failing variants and to profile finalists. They are not part of the main loop.

## 11. Prior art

- **Spire** (He, Foley, Fatahalian, SIGGRAPH 2016) covers rate placement.
- **Sitthi-amorn et al.** (SIGGRAPH Asia 2011) uses genetic programming for shader simplification with error/time Pareto fronts.
- **EnerJ** (PLDI 2011) has approximate types that need endorsement to reach precise contexts.
- **Herbie** does FP accuracy rewriting, and **Precimonious** does precision tuning.
- **Sakana AI CUDA Engineer and successors** (see axiom-compute `DESIGN.md` §1.2) show why AI-proposed rewrites need machine-checked acceptance: that discipline is the whole reason for §5's "AI proposals are hypotheses" rule.
