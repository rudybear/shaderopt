# tonemap_aces: classification

Scenarios used for ranges: tonemap_hdr_edges, tonemap_hdr_gradient (train). Sensitivity measured on tonemap_hdr_edges / pass tonemap (RGBA8_SRGB, metric color), each candidate site rounded to f16 in the CPU f32 model. Source: `lab/shaders/tonemap_aces.frag`; ids are result ids of `lab/build/spv/tonemap_aces.spv`.

## Summary

| | |
|---|---|
| candidate_sites | 48 |
| const | 6 |
| float_sites | 49 |
| pixel | 35 |
| sink_sites | 1 |
| uniform | 9 |

Rate histogram (results): const=15, pixel=36, uniform=9

## Hard sinks (zero budget)

| kind | sites | source lines |
|---|---|---|
| address | 1 | 18 |
| control | 0 |  |
| discard | 0 |  |
| convert | 0 |  |

Branch conditions and sample coordinates by source line:

- L18 `vec3 c = texture(u_scene, uv).rgb * u.exposure * u.white_balance.rgb;` -> address

## Samplers

| sampler | binding | samples | coordinate kinds |
|---|---|---|---|
| u_scene | 0 | 1 | uv_exact |

## Ranges (from the CPU model over scenario pixels)

Float results with NaN/Inf observed, or ranges beyond f16 (|x| > 65504) or below f16 normal (|x| < 6.1e-5 while nonzero):

- none

## Sensitivity to f16 rounding per site (CPU prediction)

All candidate sites at once: FLIP mean 0.0000, p99 0.0000, max 0.0000.

Most sensitive sites (avoid or budget carefully):

| id | op | rate | line | source | FLIP mean | FLIP p99 |
|---|---|---|---|---|---|---|
| %51 | OpImageSampleImplicitLod | pixel | 18 | `vec3 c = texture(u_scene, uv).rgb * u.exposure * u.white_bal` | 0.0162 | 0.0228 |
| %52 | OpVectorShuffle | pixel | 18 | `vec3 c = texture(u_scene, uv).rgb * u.exposure * u.white_bal` | 0.0162 | 0.0228 |
| %97 | OpLoad | uniform | 23 | `o = vec4(pow(c, vec3(1.0 / u.gamma)), 1.0);` | 0.0162 | 0.0228 |
| %98 | OpFDiv | uniform | 23 | `o = vec4(pow(c, vec3(1.0 / u.gamma)), 1.0);` | 0.0162 | 0.0228 |
| %99 | OpCompositeConstruct | uniform | 23 | `o = vec4(pow(c, vec3(1.0 / u.gamma)), 1.0);` | 0.0162 | 0.0228 |
| %19 | OpFAdd | pixel | 15 | `return clamp((x * (a * x + b)) / (x * (c * x + d) + e), 0.0,` | 0.0162 | 0.0228 |
| %60 | OpLoad | uniform | 18 | `vec3 c = texture(u_scene, uv).rgb * u.exposure * u.white_bal` | 0.0000 | 0.0000 |
| %61 | OpVectorTimesScalar | pixel | 18 | `vec3 c = texture(u_scene, uv).rgb * u.exposure * u.white_bal` | 0.0000 | 0.0000 |
| %65 | OpLoad | uniform | 18 | `vec3 c = texture(u_scene, uv).rgb * u.exposure * u.white_bal` | 0.0000 | 0.0000 |
| %66 | OpVectorShuffle | uniform | 18 | `vec3 c = texture(u_scene, uv).rgb * u.exposure * u.white_bal` | 0.0000 | 0.0000 |
| %67 | OpFMul | pixel | 18 | `vec3 c = texture(u_scene, uv).rgb * u.exposure * u.white_bal` | 0.0000 | 0.0000 |
| %70 | OpLoad | pixel | 19 | `float lum = dot(c, vec3(0.2126, 0.7152, 0.0722));` | 0.0000 | 0.0000 |

Sites whose f16 rounding changes no output code at all: 36 of 42 (free demotion candidates): %60, %61, %65, %66, %67, %70, %75, %76, %77, %78, %80, %81, %83, %86, %87, %88, %90, %91, %94, %100, %101, %102, %103, %104, %13, %15, %16, %20, %21, %23, %24, %27, %28, %31, %32, %37

## Pass graph

This shader never consumes another pass's output in the current scenarios.

## Rate by source line

| line | source | const | uniform | pixel |
|---|---|---|---|---|
| 15 | `return clamp((x * (a * x + b)) / (x * (c * x + d) + e), 0.0, 1.0);` | 5 | 0 | 13 |
| 18 | `vec3 c = texture(u_scene, uv).rgb * u.exposure * u.white_balance.rgb;` | 2 | 4 | 5 |
| 19 | `float lum = dot(c, vec3(0.2126, 0.7152, 0.0722));` | 0 | 0 | 2 |
| 20 | `c = mix(vec3(lum), c, 1.0) ;` | 1 | 0 | 4 |
| 21 | `c = pow(max(c, vec3(0.0)), vec3(u.contrast));` | 1 | 2 | 3 |
| 22 | `c = aces(c);` | 0 | 0 | 2 |
| 23 | `o = vec4(pow(c, vec3(1.0 / u.gamma)), 1.0);` | 1 | 3 | 6 |
