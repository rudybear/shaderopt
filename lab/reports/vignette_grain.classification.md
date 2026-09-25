# vignette_grain: classification

Scenarios used for ranges: vignette_gradient (train). Sensitivity measured on vignette_gradient / pass vignette (RGBA8, metric color), each candidate site rounded to f16 in the CPU f32 model. Source: `lab/shaders/vignette_grain.frag`; ids are result ids of `lab/build/spv/vignette_grain.spv`.

## Summary

| | |
|---|---|
| candidate_sites | 58 |
| const | 3 |
| float_sites | 59 |
| pixel | 46 |
| sink_sites | 1 |
| uniform | 11 |

Rate histogram (results): const=21, pixel=47, uniform=11

## Hard sinks (zero budget)

| kind | sites | source lines |
|---|---|---|
| address | 1 | 21 |
| control | 0 |  |
| discard | 0 |  |
| convert | 0 |  |

Branch conditions and sample coordinates by source line:

- L21 `vec3 c = texture(u_src, uv).rgb;` -> address

## Samplers

| sampler | binding | samples | coordinate kinds |
|---|---|---|---|
| u_src | 0 | 1 | uv_exact |

## Ranges (from the CPU model over scenario pixels)

Float results with NaN/Inf observed, or ranges beyond f16 (|x| > 65504) or below f16 normal (|x| < 6.1e-5 while nonzero):

- none

## Sensitivity to f16 rounding per site (CPU prediction)

All candidate sites at once: FLIP mean 0.1037, p99 0.1500, max 0.1941.

Most sensitive sites (avoid or budget carefully):

| id | op | rate | line | source | FLIP mean | FLIP p99 |
|---|---|---|---|---|---|---|
| %43 | OpFMul | pixel | 18 | `return fract((p3.x + p3.y) * p3.z);` | 0.1051 | 0.1508 |
| %19 | OpVectorTimesScalar | pixel | 16 | `vec3 p3 = fract(vec3(p.xyx) * 0.1031);` | 0.0332 | 0.0810 |
| %99 | OpFAdd | pixel | 24 | `float g = hash(uv * 1024.0 + vec2(u.time)) - 0.5;` | 0.0331 | 0.0806 |
| %16 | OpLoad | pixel | 16 | `vec3 p3 = fract(vec3(p.xyx) * 0.1031);` | 0.0331 | 0.0806 |
| %17 | OpVectorShuffle | pixel | 16 | `vec3 p3 = fract(vec3(p.xyx) * 0.1031);` | 0.0331 | 0.0806 |
| %92 | OpLoad | pixel | 24 | `float g = hash(uv * 1024.0 + vec2(u.time)) - 0.5;` | 0.0331 | 0.0806 |
| %94 | OpVectorTimesScalar | pixel | 24 | `float g = hash(uv * 1024.0 + vec2(u.time)) - 0.5;` | 0.0331 | 0.0806 |
| %30 | OpFAdd | pixel | 17 | `p3 += dot(p3, p3.yzx + 33.33);` | 0.0328 | 0.0805 |
| %27 | OpDot | pixel | 17 | `p3 += dot(p3, p3.yzx + 33.33);` | 0.0328 | 0.0802 |
| %29 | OpCompositeConstruct | pixel | 17 | `p3 += dot(p3, p3.yzx + 33.33);` | 0.0328 | 0.0802 |
| %26 | OpFAdd | pixel | 17 | `p3 += dot(p3, p3.yzx + 33.33);` | 0.0327 | 0.0802 |
| %20 | OpExtInst/Fract | pixel | 16 | `vec3 p3 = fract(vec3(p.xyx) * 0.1031);` | 0.0325 | 0.0801 |

Sites whose f16 rounding changes no output code at all: 4 of 55 (free demotion candidates): %68, %85, %97, %98

## Pass graph

This shader never consumes another pass's output in the current scenarios.

## Rate by source line

| line | source | const | uniform | pixel |
|---|---|---|---|---|
| 16 | `vec3 p3 = fract(vec3(p.xyx) * 0.1031);` | 0 | 0 | 4 |
| 17 | `p3 += dot(p3, p3.yzx + 33.33);` | 1 | 0 | 8 |
| 18 | `return fract((p3.x + p3.y) * p3.z);` | 3 | 0 | 6 |
| 21 | `vec3 c = texture(u_src, uv).rgb;` | 0 | 1 | 3 |
| 22 | `vec2 d = (uv - u.center) * vec2(u.aspect, 1.0);` | 2 | 3 | 3 |
| 23 | `float v = 1.0 - smoothstep(u.radius, u.radius + u.softness, length(d))` | 3 | 4 | 4 |
| 24 | `float g = hash(uv * 1024.0 + vec2(u.time)) - 0.5;` | 1 | 2 | 5 |
| 25 | `c = c * v + g * u.grain_amount;` | 1 | 1 | 7 |
| 26 | `o = vec4(clamp(c, 0.0, 1.0), 1.0);` | 2 | 0 | 6 |
