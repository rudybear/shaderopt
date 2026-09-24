# Feature Request: axiom-compute — fragment-shader profile for the shader lab

## Target
- [x] axiom-compute (`~/sources/axiom-compute`), NOT the CPU AXIOM compiler
- [ ] c2axiom

## Requesting project
shaderopt (`~/sources/shaderopt`), the AXIOM Shader Lab. See `AXIOM_SHADER_LAB.md` §2 and `lab/DISCOVERY.md` §2.

## Problem
The lab's design is "AXIOM is the lab, SPIR-V is the artifact": lift fragment-shader SPIR-V into an AXIOM IR that maps 1:1 to SPIR-V result IDs, run analyses and typed edit-ops there, patch the edits back into SPIR-V, and verify on real GPUs. A read-only audit of axiom-compute on 2026-09-24 (555 tests passing, 2 ignored) found that none of the pieces this needs exist:

1. No SPIR-V input path. rspirv is used only as an emitter (`crates/axc-codegen/src/emit.rs`); every `load_words` hit is in tests.
2. No MIR. `grep -rni "\bmir\b" crates/` is empty. HIR (`crates/axc-hir/src/hir.rs`, `expr.rs`) is a typechecked tree coupled to `.axc` surface syntax. The autotuner operates on HIR (`crates/axc-optimize/src/enumerator.rs:154`).
3. Scalars only. `ScalarTy` (`crates/axc-hir/src/ty.rs:16`) has no vec2/3/4 or GLSL matrix; `matrix[T,M,N,use]` is KHR cooperative-matrix with no per-element access. The only `OpTypeVector` emitted is the internal `uvec3` for invocation IDs.
4. Compute-only execution model. `GLCompute` is hardcoded (`crates/axc-codegen/src/emit.rs:529,549,1067`). No Fragment/Vertex, `Location`, `Output` storage class, `OpKill`, or derivatives.
5. GLSL.std.450: one instruction (`Exp`, `crates/axc-hir/src/ext_inst.rs:29-36`). The import/caching mechanism (`body.rs:233-240`) and `emit_exp` are reusable.
6. No images, samplers, or textures. `param::Ty` is Scalar|Buffer with "images deferred" (`crates/axc-hir/src/param.rs:17`).
7. No rewrite/pass infrastructure (no folding, CSE, DCE, algebraic simplification), no pass manager; spirv-opt never runs.
8. No CPU interpreter; correctness is GPU-vs-GPU plus hand-written per-kernel oracles (`crates/axc-driver/src/rewrite_verify.rs:701`).
9. f16 is storage-only: an f16 float literal is a codegen error (`crates/axc-codegen/src/body.rs:1609-1613`); no `RelaxedPrecision` or precision annotations.
10. No patch-back into an existing SPIR-V module; output is whole-module emission.
11. Annotation set (`crates/axc-hir/src/lower.rs:~85-215`) is `@kernel @workgroup @intent @complexity @precondition @postcondition @subgroup_uniform @cooperative_matrix @strict @strategy @optimization_log`. `@equiv_fp_tol` is not parsed (`DESIGN.md:1429`); `@coalesced`/`@occupancy` are rejected (`lower.rs:1581`). No `@rate`, `@range`, `@tolerance`, `@sink`.

## Why
Without 1 through 4 there is no IR to lift into; the lab cannot represent a single fragment shader in axiom-compute today. Items 5 through 11 are needed for the analyses and edit-ops the brief specifies.

## Impact
Blocks M1 ("faithful lift") entirely. Every shader in the lab corpus and every user-supplied shader is affected.

## Reproduction
A minimal fragment shader that axiom-compute cannot represent:
```glsl
#version 450
layout(location = 0) in vec2 uv;
layout(location = 0) out vec4 o;
layout(set = 0, binding = 0) uniform sampler2D tex;
void main() { vec3 c = texture(tex, uv).rgb; o = vec4(pow(c, vec3(1.0/2.2)), 1.0); }
```
Needs: Fragment execution model, Location I/O, `OpTypeImage`/`OpTypeSampledImage`, vec2/vec3/vec4, `OpImageSampleImplicitLod`, `OpVectorShuffle`, GLSL.std.450 `Pow`. None exist.

## Proposed options (the lab needs the guardian's verdict on which is right for axiom-compute)

**Option A — lab-local shader IR, upstream only what generalizes.** The lab builds its own Rust crate on rspirv's `dr::Module` (already a 1:1, result-ID-preserving representation with load and assemble), with a fragment-profile CPU interpreter, a pass framework, and typed edit-ops. axiom-compute is untouched now. Later, proven pieces are proposed individually through the guardian: the GLSL.std.450 emitters (reusing `get_or_emit_glsl450_set`), a vector type in `ScalarTy`/codegen, f16 arithmetic, and the annotation vocabulary `@rate/@range/@tolerance/@sink` plus a real `@equiv_fp_tol`. axiom-compute's `TolerancePolicy` (`rewrite_verify.rs:55`), `verify_rewrite` verdict schema, MCP tool shapes, and `@strategy` hole semantics are adopted by the lab as-is so a later merge is natural.

**Option B — grow a shader profile inside axiom-compute first.** A SPIR-V lifter, a MIR, vector/matrix/image types, Fragment execution model, GLSL.std.450 coverage, pass infrastructure and an interpreter, all through the 7-agent pipeline before M1 can start. This contradicts `CLAUDE.md:8` ("NOT a shader/graphics language") and is many milestones of work with no consumer other than the lab until then.

**Option C — B, but scoped to a new `axc-shader` crate** that depends on nothing in HIR, so compute stays untouched. Same cost as B for the lab, cleaner separation.

The lab's recommendation is A, with C as the eventual upstream shape.

## Tests the eventual upstream change would need (minimum)
1. Lift-then-assemble round trip is byte-identical for every SPIR-V in the lab corpus (header excepted).
2. GLSL.std.450 emitters validated by `spirv-val` and bit-compared against glslang output for each instruction and vector width.
3. Vector types: swizzle, shuffle, composite extract/insert, arithmetic, all validated.
4. f16 literal and arithmetic emission with `Float16` capability, validated, GPU-executed on NVIDIA and Lavapipe.
5. Annotation parsing: `@tolerance`, `@rate`, `@range`, `@sink` accepted, unknown still rejected; backward compatibility for all existing `.axc` examples.
6. Existing 555 tests unchanged.

## Cross-project impact
Only shaderopt today. A vector type, GLSL.std.450 coverage and f16 arithmetic would also benefit compute kernels (activation functions, packed math) in axiom-compute's llama.cpp beachhead.

## Human approval required: YES
