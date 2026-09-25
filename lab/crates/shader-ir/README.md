# shader-ir

The shader lab's CPU model: a lossless SPIR-V IR on `rspirv` plus an interpreter for fragment
shaders. This is the "faithful lift" half of milestone M1 (see `lab/AXIOM_REQUESTS.md`, "Agent 5,
workaround designer": the IR *is* `rspirv::dr::Module`, never a second copy).

```
cd lab/crates/shader-ir
cargo build --release
cargo test                     # needs the pinned glslang; tests skip (not fail) without it
```

## Design

### `lift` (src/lift.rs)

`Lifted::load(words)` wraps `rspirv::dr::Module` (rspirv 0.12, the same pin as axiom-compute)
with side tables built from it:

| table | contents |
|---|---|
| `types` | id -> `Type` (scalar, vector, matrix, struct, array, pointer, image, sampled image, sampler, function) |
| `constants` | id -> raw bit pattern / composite ids / null / undef (typed by the interpreter) |
| `defs`, `uses`, `result_types` | def-use: id -> `Site` (global index, or function/block/instruction indices) |
| `functions`, `label_index` | block order per function, label -> (function, block), `headers` = selection/loop merge info (`OpSelectionMerge`, `OpLoopMerge` with continue target) |
| `entry_points`, `variables` | execution model, interface ids; every module-scope `OpVariable` with Location, Binding, DescriptorSet, Block, BuiltIn, RelaxedPrecision, initializer |
| `names`, `member_names`, `decorations`, `member_decorations` | `OpName`/`OpMemberName`, every `OpDecorate`/`OpMemberDecorate` (Offset, ArrayStride, MatrixStride, ... are queried from here) |

Edit passes mutate `lifted.module` in place and call `reanalyze()`; `assemble()` emits words.
The lift never reorders instructions, so the empty edit round trip is body-identical.

**Header word 2.** The only difference between input and `assemble()` output is header word 2,
the generator magic: `rspirv` writes its own id (`0xf0000`) in place of the producer's (glslang
15.3.0: `0x8000b`). Magic, version, bound and schema are preserved. `shader-ir roundtrip` prints
this diff and exits 0 when the body (everything after the 5 header words) is identical. The lab's
`lab/probes/rspirv_roundtrip` established this on `-V`, `-V -Os` and `spirv-opt -O` output; the
`roundtrip` tests keep it as the M1 gate on the fixture and on every `lab/shaders/*.frag`.

### `interp` (src/interp/)

The interpreter executes the `Fragment` entry point of a `dr::Module` directly (no lowering).

* **Values** (`value.rs`): `Bool`, `I32`, `U32`, `F(f64)` (every float type; the stored f64 is
  always exactly representable at the type's precision), `V(Vec<Value>)` for vectors, matrices
  (column vectors), structs and arrays, `Ptr { root variable id, access path }`, and image
  handles. Only 32-bit integers are supported; 16/64-bit integers are a clear error.
* **Invocation** (`exec.rs`): a resumable state machine with an explicit call stack (no recursion
  in the shader; helper functions with `in`/`out`/`inout` parameters work as glslang emits them).
  `OpPhi` is evaluated simultaneously at block entry. `OpVariable` in a function allocates
  (zero-initialized unless an initializer is given) each time the function is entered.
* **Numeric modes** (`Mode`):
  * `f64`: every float operation is evaluated in f64; inputs are widened.
  * `f32`: IEEE f32 for 32-bit float types (Rust `f32` arithmetic and `libm` functions), f16/f64
    for 16/64-bit types: exactly what the SPIR-V says.
  * `f16`: a **prediction** of a demoted shader. Every float-typed *computing* instruction
    (arithmetic, `GLSL.std.450`, conversions, image samples, derivatives) is computed in f32 and
    its result rounded to f16 (round to nearest even via `half::f16`), then widened again. Loads,
    stores, composite construction and shuffles move values unchanged; uniform values are f32.
    The double rounding (f32 then f16) and the f32 intermediate are approximations of real f16
    hardware, which is why the lab labels this mode `predicted`.
* **Compound functions** follow the GLSL definitions step by step with a rounding after every step
  (`mix = x*(1-a) + y*a`, `smoothstep`, `mod = x - y*floor(x/y)`, `normalize = v *
  inversesqrt(dot(v,v))`, `length = sqrt(dot(v,v))`, `reflect`, `refract`, `dot` = products then a
  left-to-right sum). GPUs may fuse or reorder these, so they are ULP-level approximations.
  `Fma` uses a real fused multiply-add. `Determinant`/`MatrixInverse` are computed in f64 and
  rounded once. Integer division by zero yields 0 (undefined on a GPU).

#### Derivatives: 2x2 quads

Pixels are evaluated in 2x2 quads aligned to even coordinates, like a GPU. The four invocations
of a quad run in lockstep at every derivative instruction: each lane runs until it reaches
`OpDPdx`/`OpDPdy`/`OpFwidth` (or a Fine/Coarse variant), the driver collects the operand from all
four lanes, and

* `dFdx` = right lane minus left lane of the row (both lanes of the row get the same value),
* `dFdy` = bottom lane minus top lane of the column,
* `fwidth` = `|dFdx| + |dFdy|`,
* Fine variants use the requesting lane's own row/column; Coarse variants use the quad's top row
  and left column. With 2x2 quads this only differs when a lane is dead.

All four lanes must reach the same derivative instruction (uniform control flow within the quad),
otherwise evaluation fails with an error. A lane that was already discarded (`OpKill`) has no
operand value: the derivative falls back to 0 and is counted in `dead_derivatives` (a GPU gives
an undefined value there). `OpDemoteToHelperInvocation` keeps the lane running as a helper.
Quads that straddle the image edge (odd sizes) are padded with invocations outside the image whose
outputs are dropped. Implicit-lod sampling (`texture()`) does not need derivatives because the lab
has no mipmaps (lod 0 always).

#### Sampling (image.rs)

Images are RGBA float32 `(H, W, 4)`. The sampler models Vulkan `VK_FILTER_LINEAR`/`NEAREST` with
no mipmaps and clamp-to-edge; texel `i` covers `[i, i+1)` so its center is `(i + 0.5) / W`:

```
u = s * W,  v = t * H
nearest:  i = floor(u), j = floor(v), clamped to the image
linear:   i0 = floor(u - 0.5), j0 = floor(v - 0.5), a = (u - 0.5) - i0, b = (v - 0.5) - j0
          result = (1-a)(1-b) T[i0,j0] + a(1-b) T[i0+1,j0] + (1-a) b T[i0,j0+1] + a b T[i0+1,j0+1]
          (every texel index clamped after adding a ConstOffset/Offset)
```

Weights and the blend are computed in f64 and the result is rounded to the numeric mode. Real
GPUs use fixed-point weights (Vulkan guarantees at least 8 fractional bits, `subTexelPrecisionBits`;
NVIDIA reports 8) and unspecified blend precision, so the lab treats the CPU sampler as an
approximation. `--sampler-weight-bits N` truncates `a` and `b` to N fractional bits
(`floor(a * 2^N) / 2^N`, i.e. fixed-point conversion by truncation) so the approximation can be
tightened once measured on a device; the rounding direction of that conversion is a hypothesis to
verify per vendor, not a spec guarantee. `texelFetch` is exact; out-of-bounds fetches return 0
(robust-access behavior). `textureSize` returns the bound image size, `textureQueryLevels` 1.
Explicit lod, bias and gradient operands are ignored (lod 0). Only non-arrayed, single-sampled 2D
float images are supported.

#### Inputs, uniforms, outputs

* `Location 0` input must be a `vec2`: `uv = ((x + 0.5) / W, (y + 0.5) / H)`, `(0, 0)` at the
  top-left, the runner's fullscreen-triangle convention. `gl_FragCoord = (x + 0.5, y + 0.5, 0.5,
  1.0)`, `gl_FrontFacing = true`. Other `Location` inputs are an error unless a constant is
  given with `--input name=value`.
* The uniform block (a `Block`-decorated `Uniform` or `PushConstant` variable) is filled member by
  member from `--uniform name=value`, scenario syntax: `1.0`, `3`, `true`, `[1,2,3]`, 16 numbers
  column-major for a `mat4`; any type is filled leaf by leaf in declaration order. A missing
  member, or a name that is not a member, is an error listing the members with their offsets.
* Samplers: `--sampler name=path.npy[:nearest]`, matched by the `OpName` of the
  `UniformConstant` variable. Missing sampler = error listing the sampler variables.
* **Stripped names.** `glslang -g0` (the compile line in `CONTRACTS.md`) removes `OpName` and
  `OpMemberName`, so name matching is impossible on such modules. The keys `binding<B>` (samplers),
  `member<i>` and `offset<O>` (uniform members) are therefore always accepted, and the error
  messages name them. The lab should either drop `-g0` from the pinned compile line or drive
  `shader-ir` with these keys (the same problem hits SPIRV-Reflect's member names in the runner).
* Outputs: every `Output` variable with a `Location`; `--out` writes `--out-location` (default 0).
  Discarded pixels are `--discard-value` (default NaN) in all channels. Outputs with fewer than
  four components are padded with 0; integer outputs are converted to float. Builtin outputs
  (`gl_FragDepth`) are accepted and ignored.

### Supported instructions

Memory: `OpVariable` (Function, Private, Input, Output, Uniform, PushConstant, UniformConstant),
`OpLoad`, `OpStore`, `OpCopyMemory`, `OpAccessChain`, `OpInBoundsAccessChain`.
Composites: `OpCompositeConstruct/Extract/Insert`, `OpVectorShuffle`,
`OpVectorExtractDynamic/InsertDynamic`, `OpCopyObject`, `OpCopyLogical`, `OpUndef`, `OpTranspose`.
Arithmetic: `OpFAdd/FSub/FMul/FDiv/FRem/FMod/FNegate`, `OpIAdd/ISub/IMul/SDiv/UDiv/SRem/SMod/UMod/SNegate`,
`OpDot`, `OpVectorTimesScalar`, `OpMatrixTimesScalar`, `OpVectorTimesMatrix`, `OpMatrixTimesVector`,
`OpMatrixTimesMatrix`, `OpOuterProduct`. Bits: `OpBitwiseAnd/Or/Xor`, `OpNot`, `OpShiftLeftLogical`,
`OpShiftRightLogical/Arithmetic`, `OpBitCount`, `OpBitReverse`, `OpBitFieldInsert/SExtract/UExtract`.
Comparisons and logic: all `OpFOrd*`/`OpFUnord*`, `OpIEqual/INotEqual`, `OpS*/OpU*` comparisons,
`OpLogicalAnd/Or/Not/Equal/NotEqual`, `OpSelect` (scalar or vector condition), `OpAny`, `OpAll`,
`OpIsNan`, `OpIsInf`. Conversions: `OpConvertFToS/FToU/SToF/UToF`, `OpFConvert`, `OpSConvert`,
`OpUConvert`, `OpQuantizeToF16`, `OpBitcast` (same component count).
Control flow: `OpBranch`, `OpBranchConditional`, `OpSwitch`, `OpSelectionMerge`, `OpLoopMerge`,
`OpPhi`, `OpFunctionCall`, `OpReturn`, `OpReturnValue`, `OpKill`, `OpTerminateInvocation`,
`OpDemoteToHelperInvocation`, `OpIsHelperInvocationEXT`, `OpUnreachable` (error if executed),
`OpNop`, `OpLine`, `OpNoLine`.
Images: `OpSampledImage`, `OpImage`, `OpImageSampleImplicitLod`, `OpImageSampleExplicitLod`,
`OpImageFetch`, `OpImageQuerySize`, `OpImageQuerySizeLod`, `OpImageQueryLevels`.
Derivatives: `OpDPdx/DPdy/Fwidth` and the Fine/Coarse variants.
`GLSL.std.450`: everything except `IMix`, `PackDouble2x32`, `UnpackDouble2x32` and
`InterpolateAt*` (Round, RoundEven, Trunc, FAbs, SAbs, FSign, SSign, Floor, Ceil, Fract, Radians,
Degrees, Sin, Cos, Tan, Asin, Acos, Atan, Sinh, Cosh, Tanh, Asinh, Acosh, Atanh, Atan2, Pow, Exp,
Log, Exp2, Log2, Sqrt, InverseSqrt, Determinant, MatrixInverse, Modf, ModfStruct, F/U/S Min, Max,
Clamp, FMix, Step, SmoothStep, Fma, Frexp, FrexpStruct, Ldexp, Pack/Unpack Half2x16, Unorm4x8,
Snorm4x8, Unorm2x16, Snorm2x16, Length, Distance, Cross, Normalize, FaceForward, Reflect, Refract,
FindILsb, FindSMsb, FindUMsb, NMin, NMax, NClamp).

Anything else fails with an error naming the opcode, the result id and the shader
(`<shader>: in OpImageSampleProjImplicitLod (%17): unsupported opcode ...`), never a silent
wrong value. Known unsupported: projective/depth-compare/gather sampling, cube/3D/array/multisample
images, integer images, `OpSpecConstantOp`, runtime arrays and storage buffers, 8/16/64-bit
integers, `OpImageQueryLod`, other extended instruction sets.

### `analysis` (src/analysis/): the M2 static analysis

`shader-ir analyze` produces the `analysis.json` of `CONTRACTS.md` ("M2: analysis and rewrite
CLIs"): one entry per instruction of every function body (parameters and labels included, in
layout order; `index` is the position in the function counting all of them, `block` is null for
parameters), with the result `id` (0 when there is none), `op`, `ext` (the `GLSL.std.450` name),
`type` (`f32`, `vec3<f32>`, `mat4<f32>`, `i32`, `bool`, `ptr(Function, f32)`, `array<f32, 4>`,
`sampler2D`), `func`, `line`, `name` (`OpName`), `operands` (id operands only), `rate`, `sinks`
and `range`. A top-level `functions` array adds each function's `call_sites` count.

* **Rates** (`rate.rs`): forward dataflow over `const < uniform < pixel`. Constants and pointers
  are `const`; a load from a `Uniform`/`PushConstant`/`UniformConstant` variable is `uniform`;
  a load from an `Input` variable (Location inputs, `gl_FragCoord`, any builtin), an image
  sample/fetch/gather/query result and a derivative are `pixel`; everything else joins its
  operands. A `Function`/`Private` variable has the join of every store to it (all stores are
  treated as reaching) joined with the *control rate* of the storing block, the join of the
  branch conditions the block is control dependent on (post-dominator based, `cfg.rs`); an
  `OpPhi` joins its incoming values with the control rate and branch condition of each
  predecessor. This iterates to a fixed point per function and over the module, so
  `for (int i = -4; i <= 4; ++i)` comes out `const` and `exp(-0.5*x*x/(sigma*sigma))` with a
  uniform `sigma` comes out `uniform`, while `s = 0; if (uv.x < 0.5) s = 1;` makes `s` pixel.
  Calls are instantiated per call site with the argument rates (pointer arguments alias the
  caller's variables); a callee's instructions are reported once with the join over call sites.
  Functions never called are analyzed with `pixel` parameters.
* **Sinks** (`sinks.rs`): backward marking. Seeds: `address` = the coordinate and trailing image
  operands (bias, lod, gradient, offset) of every sample/fetch/gather, every `OpAccessChain`
  index, the index of `OpVectorExtractDynamic`/`OpVectorInsertDynamic`; `control` = the
  condition of `OpBranchConditional`/`OpSelect` and the selector of `OpSwitch`; `discard` = a
  branch condition one of whose arms dominates a block with `OpKill`/`OpTerminateInvocation`/
  `OpDemoteToHelperInvocation` or a call to a function that may kill; `convert` = the operand of
  `OpConvertFToS/U`. Marks propagate to the operands of pure instructions (arithmetic,
  `GLSL.std.450`, comparisons, conversions, composites, shuffles, selects, phis, derivatives)
  and stop at loads of `Uniform`/`PushConstant`/`Input`/`UniformConstant` variables (the load is
  marked, its pointer is not) and at image instructions (the sample result is marked, its
  coordinate is not). A load of a `Function`/`Private` variable propagates to every value
  stored to that variable (glslang without `-O` routes every local through a variable, so the
  chain `dir = clamp(...) * u.texel; texture(t, uv + dir * k)` is an address chain all the way
  back), parameters propagate to the arguments at every call site and call results to the
  callee's `OpReturnValue`.
* **Samplers**: for every `UniformConstant` sampled image, each sample/fetch instruction that
  reads it (through `OpSampledImage`, loads, parameters), its coordinate id and `coord_kind`:
  `uv_exact` when the coordinate is a load of the Location 0 input (or a construct/shuffle that
  reproduces it, or a local variable assigned exactly once from it), `uv_offset` when it is
  `uv +/- e` with `e` of rate `const` or `uniform` (`offset` is `e` as a vec2 when it folds to
  constants, negated for `-`), otherwise `other`.
* **Lines** (`lines.rs`): with `--debug-spv x.g.spv` (`glslang -V -g`), the k-th body
  instruction of the measured build (after dropping `OpLine`/`OpNoLine` from the debug build)
  takes the line of the last `OpLine` before its debug twin. The opcode sequences (and
  `GLSL.std.450` numbers) must match exactly, otherwise `analyze` fails naming the first
  mismatch. Ids in the output are always the measured build's.
* **Summary**: `pixel`/`uniform`/`const` count value-producing instructions (a result id whose
  type is neither a pointer nor void); `float_sites` are those with a float scalar/vector/matrix
  type, `sink_sites` those with any sink, `candidate_sites` float sites without sinks.

#### Ranges and f16 sites (interpreter)

`eval --profile ranges.json [--stride N]` records, for every float-typed result id (scalars,
and every component of vectors/matrices), `min`/`max` over finite values, `nan` and `inf`
component counts and `samples` (evaluations of the instruction, so a value inside a 9-tap loop
has 9 samples per pixel). `--stride N` evaluates every N-th quad in each dimension (quads at
`(2iN, 2jN)`, whole, so derivatives stay exact); unevaluated pixels get the discard value in the
output image. Accumulators live per lane and are merged per quad row and once at the end.
`analyze --ranges ranges.json` attaches them as `range` (null for ids without samples).

`eval --f16-sites 57,58 | --f16-all` rounds the listed float-typed results (or all of them) to
f16 (round to nearest even through `half::f16`) right after they are computed, in whatever
numeric mode is selected, and continues with the rounded value. This is the prediction for
demoting those sites to `RelaxedPrecision` / explicit f16: the site computes in f32 (or the
mode's precision) and only its stored result loses precision. Sites feeding address/control
sinks are rounded like any other when listed; the caller chooses the list (typically the
`candidate_sites` of `analyze`). Listing an id that is not a float-typed result is an error.

## CLI (see `lab/CONTRACTS.md`)

```
shader-ir roundtrip <in.spv> [--out out.spv]
    # exit 0 iff the body after the header is identical; prints the header diff
shader-ir eval --spv pass.spv --width W --height H --mode f32|f64|f16 \
    --sampler name=path.npy[:nearest] ... --uniform name=value ... \
    --out out.npy [--out-location N] [--discard-value nan] [--sampler-weight-bits N] \
    [--input name=value] [--threads N]
shader-ir info <in.spv>
    # entry point, interface variables (names, locations, bindings), uniform block layout
    # (member offsets, strides), opcode histogram including GLSL.std.450 names
shader-ir analyze --spv x.spv [--debug-spv x.g.spv] [--ranges ranges.json] --out analysis.json
    # M2 analysis JSON: rates, sinks, samplers/coord_kind, outputs, summary, lines, ranges
shader-ir eval ... --profile ranges.json [--stride N]     # per-result float ranges
shader-ir eval ... --f16-sites 57,58 | --f16-all          # round listed results to f16
```

How the lab uses it: `lab` compiles the baseline and each variant, runs `roundtrip` as the M1
gate, `info` to classify shaders (opcode histogram, bindings), and `eval` to produce the CPU
reference (`f64`), the `predicted` f32 image to compare against the desktop GPU, and the `f16`
prediction for precision-demotion variants. Images cross process boundaries as `.npy`
(`npy.rs`: float32, C order, `(H, W, 4)`; the reader also accepts 1-3 channels and `<f8`).

## Performance

Rows of quads are distributed over threads with rayon (`--threads` to limit); a quad never
splits. Measured on the tonemap fixture (two `texture()` calls plus a 5-tap loop, `aces`,
`pow`, `discard`) at 1920x1080 in release mode, 32-core host: about 1.0 s in each mode (f32,
f64, f16) with all cores, about 15 s with `--threads 1`.

## Tests

`cargo test` compiles GLSL with the pinned glslang
(`~/sources/igl/third-party/deps/src/glslang/build/StandAlone/glslang`) and validates
re-assembled modules with `spirv-val`; both are skipped, not failed, when absent. Covered: round
trip on the fixture (with and without `-g0`) and on every `lab/shaders/*.frag`; constant color;
uv passthrough and orientation; loops (`for`/`while`/`do`, `continue`); if/else with discard and
custom discard values; helper functions with `inout` parameters; `switch`, `mix(bvec)`, `any`;
bilinear and nearest sampling of a 2x2 image against hand-computed weights, weight quantization;
`texelFetch`/`textureSize`; `dFdx(uv.x) == 1/W` and `dFdy(uv.y) == 1/H` on every pixel including
odd sizes; derivatives next to a discarded lane; `pow/exp/mix/clamp/smoothstep/sqrt/log2/sin/atan`
against Rust f64 math in `f64` mode and Rust f32 math in `f32` mode; `f16` mode rounding
`1 + 2^-12` to 1 and overflowing to inf; matrices, integer and bit ops; private globals, arrays,
structs, swizzled stores; error messages for missing/unknown uniforms and samplers; the
stripped-name fallback keys; unsupported opcode naming; the tonemap fixture against a closed-form
hand computation; and `info` tables and the binary's output.

## M2 rewrite passes (`src/passes/`, `shader-ir rewrite`)

```
shader-ir rewrite --spv in.spv --out out.spv --passes fold,dce,cse,ident,unroll,divconst,powspec,select \
    --ops ops.json [--max-unroll 16] [--max-select-arm 32] [--only-op ID] [--exact-only]
```

The passes edit `Lifted::module` in place (`reanalyze()` after each batch). Untouched
instructions keep their result ids; new results take fresh ids above the old bound (the header
bound is updated). The output is validated in-process with the `spirv-tools` crate; a failure is
an error naming the diagnostic. Passes are applied in the listed order; the cleanup passes among
them (`fold,dce,cse,ident`) are repeated to a fixed point (<= 10 rounds) after every other pass
and at the end. `--only-op ID` restricts every pass except `dce` to the target id (a result id;
the header label for `unroll`; the header label, merge label or a phi id for `select`).
`ops.json` is the contract's list of `{"pass","class","target","replaced_by","detail"}` records;
`class` is `exact`, `ulp` or (from `demote` only) `lossy`.

| pass | edit | class |
|---|---|---|
| `fold` | arithmetic, comparisons, logic, composites (Construct/Extract/Insert/Shuffle), conversions, `OpSelect` with a constant condition, GLSL.std.450 with all-constant operands; results become deduplicated `OpConstant`/`OpConstantComposite` | `exact` when no rounding happened (the f32 result equals the f64 evaluation, so the f64 reference is unchanged); `ulp` for a rounding f32 result and always for transcendentals (`exp`, `pow`, `sin`, `sqrt`, `mix`, `length`, ...: Rust's `f32` functions on the CPU may differ from the GPU's by ULPs) |
| `dce` | unused pure results (iterated), Function variables only stored to (with their stores and access chains); interface variables and anything with side effects (stores, kills, calls, image writes, barriers, atomics) stay | `exact` |
| `cse` | identical pure instructions (opcode, type, operands, ext-inst) where the earlier dominates the later (dominator tree over the CFG); loads only from read-only storage (Uniform, UniformConstant, PushConstant, Input); image ops and derivatives excluded | `exact` |
| `ident` | `x*1`, `1*x`, `x+0`, `0+x`, `x-0`, `x/1`, `-(-x)`, `select(c,x,x)`, vector forms with splat constants, `OpVectorTimesScalar(v,1)`, integer forms; never `x*0` or `x-x` | `exact` (caveat: `x + (+0.0)` and `x - (-0.0)` map `-0.0` to `+0.0` before the rewrite and keep it after; the detail says so) |
| `unroll` | loops (`OpLoopMerge`) with one exit in the check block, an integer compare of the induction value against a constant, the induction value an `OpPhi` (init/step constants) or, as glslang `-V` emits, a Function variable stored exactly twice (constant before the header, `load +/- const` in a block that dominates the continue block); trip count `<= --max-unroll`; no inner loop (inner first), contiguous blocks, single-predecessor continue block (no `continue` from a nested `if`). The check block(s) are cloned N+1 times, the body N times, induction loads/phis become per-iteration constants, the loop structure is dropped | `exact` |
| `divconst` | `x / c -> x * (1/c)` for a constant scalar or vector `c` with finite, non-zero, normal components and reciprocals; in place, same id; the detail carries the reciprocal bits | `ulp` (`exact` when every component is a power of two) |
| `powspec` | `pow(x,2) -> x*x`, `pow(x,0.5) -> sqrt(x)`, `pow(x,1) -> x`, `pow(x,3) -> (x*x)*x` for constant scalar/splat exponents; `pow` is undefined for negative `x`, the products are not (the defined domain only widens) | `ulp` |
| `select` | if/else (`OpSelectionMerge` + `OpBranchConditional`) whose arms are single blocks of speculatable instructions (no stores, image ops, derivatives, kills, calls, loops; loads only through constant-index pointers; <= `--max-select-arm` each) both branching to the merge block whose phis are the only join points; one arm may be empty (glslang's `a || b`, `a && b`): arms move into the header, each phi becomes an `OpSelect` with the same id | `exact` |

Notes from the corpus (`lab/build/spv`): glslang builds splat divisors with `OpCompositeConstruct`,
so `divconst` on `v / 3.0` needs `fold` first (the full pipeline has it); glslang emits
`OpLogicalOr`/`OpSelect` for simple conditions, so `select` only fires on short-circuit operands
with loads (`alb.a < 0.5 || nrm.w < 0.5`); ternaries with non-trivial arms are store-based
if/else in `-V` output and are not `select` candidates; after `unroll`, values that pass through
Function variables (`float x = float(i)`) stay loads/stores, so the unrolled Gaussian weights do
not fold (a store-to-load forwarding pass would be needed). `--exact-only` skips every edit that
would be classed `ulp`, which is what the tests use for the f64 bit-identity gate.

## M3 precision demotion (`src/passes/demote.rs`, `shader-ir demote`)

```
shader-ir demote --spv in.spv --out out.spv --sites 57,58,90 --mode relaxed|f16 --ops ops.json [--group-converts]
```

`--sites` lists f32 float-typed result ids (scalars or vectors) of the input module; ids that
do not exist or are not f32-typed are an error listing them. Which sites are sensible (no
sinks, ranges inside f16) is the caller's decision (`analyze` gives `candidate_sites`, `eval
--f16-sites` the prediction). Both modes record class `lossy` ops (the value changes by design;
`exact`/`ulp` are the M2 classes).

* `--mode relaxed`: `OpDecorate %id RelaxedPrecision` per site, nothing else (pass
  `demote_relaxed`). Mobile drivers honor the hint (mediump), desktop drivers usually ignore
  it; the interpreter ignores it.
* `--mode f16` (pass `demote_f16`): each listed instruction keeps its id and computes in f16:
  the result type becomes `f16`/`vecN<f16>`; f32 operands outside the set get an `OpFConvert`
  to f16 right before the instruction (deduplicated per (operand, block); phi incoming values
  convert at the end of the predecessor block, before its merge/terminator); constant operands
  become f16 constants (`OpConstant` with the half bits, RNE, deduplicated); every use of the
  result by an instruction outside the set goes through one `OpFConvert` back to f32 placed right
  after the instruction (after the phis of the block for an `OpPhi`). `OpCapability Float16`,
  `OpTypeFloat 16`, its vectors and `Function` pointer types are added when missing. Supported:
  `OpFAdd/FSub/FMul/FDiv/FRem/FMod/FNegate`, `OpDot`, `OpVectorTimesScalar`,
  `OpCompositeConstruct/Extract/Insert`, `OpVectorShuffle`, `OpVectorExtractDynamic/InsertDynamic`,
  `OpSelect`, `OpPhi`, `OpCopyObject`, `OpConvertSToF/UToF`, the float `GLSL.std.450`
  instructions whose float operands and result change together (`Exp`, `Pow`, `FMix`,
  `SmoothStep`, `Length`, `Normalize`, ...), and `OpLoad` of a `Function` variable: listing a
  load demotes the variable (pointer type `Function f16`, every store to it converts the stored
  value, the variable and each store get their own op with `target` = the variable id), which
  requires every load of that variable to be listed and the variable to be used only by
  whole-variable loads and stores (glslang's `v.x = ...` access chains and `param` variables
  passed to calls are rejected). A listed `OpFConvert` is skipped. Rejected with a message naming
  the id and the reason: comparisons, image ops, derivatives, matrix ops, loads of interface
  storage (`Uniform`, `Input`, `PushConstant`, `UniformConstant`, `Output`, `Private`), loads
  through access chains or pointer parameters, function parameters and call results,
  mixed-signature `GLSL.std.450` ops (`Ldexp`, `Frexp`, `Modf`, pack/unpack, matrix functions),
  matrix/struct/array operands, specialization-constant operands. `demote::prune` applies the
  same rules to a candidate list and returns the survivors, so "demote everything demotable"
  is one call (the corpus test does this).
* `--group-converts`: afterwards removes `f16 -> f32 -> f16` pairs (an `OpFConvert` of an
  `OpFConvert` back to the original type when the intermediate has no other use; pass
  `group_converts`, class `exact`). One `demote` call never creates such pairs (a listed operand
  is used directly); they appear when demotion is applied incrementally to an already demoted
  module, where a site's operand is the previous site's convert back.

The interpreter needs no extension: in `f32` mode a 16-bit float type is computed at f16
precision (`Mode::prec(16)`), `OpFConvert` rounds through `half::f16`, f16 constants are read
from their half bits. Its `--f16-sites` prediction rounds only *results*; the demoted module
also rounds the f32 operands entering a region and uses f16 constants, so the two differ by
about one f16 ULP per chained operation and more under cancellation. Corpus (`tests/demote.rs`,
every `analyze` candidate that survives `prune`, `--group-converts`, 64x36, generic inputs):
`color_grade` 49 sites (10 converts in / 1 out), `fxaa` 29 (11/2), `gaussian_blur_h/v` 21
(5/1), `bloom_composite` 16 (7/3), `bloom_threshold` 33 (6/1), `deferred_lit` 39 (9/2),
`tonemap_aces` 38 (10/3), `vignette_grain` 40 (18/5); no pairs to remove (single call); the
demoted module is within 0 ULP (`color_grade`, `fxaa`, `gaussian_blur_*`), 1 (`bloom_composite`),
2 (`deferred_lit`, `tonemap_aces`), 10 (`bloom_threshold`: `lum - threshold` cancels) of the
prediction, and unbounded on `vignette_grain`, whose grain hash (`fract` of values near 1e4,
f16 spacing 8) is chaotic under any operand difference (the prediction itself is 2047 ULP from
f32 there). Rejections in the corpus are uniform/input loads, image samples, call results,
loads through the `param` pointer of `hash(vec2 p)`, and variables with an unlisted (sink)
load or a component access chain (`p3.x`, `alb.a`).
