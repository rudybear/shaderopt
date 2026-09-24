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
