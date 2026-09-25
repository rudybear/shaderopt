# color_grade: classification

Scenarios used for ranges: colorgrade_gradient, colorgrade_photo (train). Sensitivity measured on colorgrade_gradient / pass grade (RGBA8, metric color), each candidate site rounded to f16 in the CPU f32 model. Source: `lab/shaders/color_grade.frag`; ids are result ids of `lab/build/spv/color_grade.spv`.

## Summary

| | |
|---|---|
| candidate_sites | 59 |
| const | 4 |
| float_sites | 60 |
| pixel | 32 |
| sink_sites | 1 |
| uniform | 25 |

Rate histogram (results): const=19, pixel=32, uniform=25

## Hard sinks (zero budget)

| kind | sites | source lines |
|---|---|---|
| address | 1 | 18 |
| control | 0 |  |
| discard | 0 |  |
| convert | 0 |  |

Branch conditions and sample coordinates by source line:

- L18 `vec3 c = texture(u_src, uv).rgb;` -> address

## Samplers

| sampler | binding | samples | coordinate kinds |
|---|---|---|---|
| u_src | 0 | 1 | uv_exact |

## Ranges (from the CPU model over scenario pixels)

Float results with NaN/Inf observed, or ranges beyond f16 (|x| > 65504) or below f16 normal (|x| < 6.1e-5 while nonzero):

- none

## Sensitivity to f16 rounding per site (CPU prediction)

All candidate sites at once: FLIP mean 0.0075, p99 0.0244, max 0.0521.

Most sensitive sites (avoid or budget carefully):

| id | op | rate | line | source | FLIP mean | FLIP p99 |
|---|---|---|---|---|---|---|
| %108 | OpExtInst/Pow | pixel | 25 | `vec3 g = pow(max(m * u.gain.rgb + u.lift.rgb * (1.0 - m), ve` | 0.0040 | 0.0210 |
| %111 | OpLoad | pixel | 26 | `o = vec4(clamp(g, 0.0, 1.0), 1.0);` | 0.0040 | 0.0210 |
| %114 | OpExtInst/FClamp | pixel | 26 | `o = vec4(clamp(g, 0.0, 1.0), 1.0);` | 0.0040 | 0.0210 |
| %118 | OpCompositeConstruct | pixel | 26 | `o = vec4(clamp(g, 0.0, 1.0), 1.0);` | 0.0040 | 0.0210 |
| %95 | OpFAdd | pixel | 25 | `vec3 g = pow(max(m * u.gain.rgb + u.lift.rgb * (1.0 - m), ve` | 0.0037 | 0.0209 |
| %98 | OpExtInst/FMax | pixel | 25 | `vec3 g = pow(max(m * u.gain.rgb + u.lift.rgb * (1.0 - m), ve` | 0.0037 | 0.0209 |
| %86 | OpFMul | pixel | 25 | `vec3 g = pow(max(m * u.gain.rgb + u.lift.rgb * (1.0 - m), ve` | 0.0038 | 0.0208 |
| %79 | OpExtInst/FMix | pixel | 24 | `m = mix(vec3(lum), m, u.saturation);` | 0.0038 | 0.0208 |
| %81 | OpLoad | pixel | 25 | `vec3 g = pow(max(m * u.gain.rgb + u.lift.rgb * (1.0 - m), ve` | 0.0038 | 0.0208 |
| %116 | OpCompositeExtract | pixel | 26 | `o = vec4(clamp(g, 0.0, 1.0), 1.0);` | 0.0024 | 0.0208 |
| %42 | OpFMul | pixel | 21 | `c *= warm;` | 0.0037 | 0.0205 |
| %20 | OpImageSampleImplicitLod | pixel | 18 | `vec3 c = texture(u_src, uv).rgb;` | 0.0037 | 0.0205 |

Sites whose f16 rounding changes no output code at all: 0 of 55 (free demotion candidates): 

## Pass graph

This shader never consumes another pass's output in the current scenarios.

## Rate by source line

| line | source | const | uniform | pixel |
|---|---|---|---|---|
| 18 | `vec3 c = texture(u_src, uv).rgb;` | 0 | 1 | 3 |
| 20 | `vec3 warm = vec3(1.0 + 0.1 * u.temperature, 1.0, 1.0 - 0.1 * u.tempera` | 2 | 7 | 0 |
| 21 | `c *= warm;` | 0 | 1 | 2 |
| 22 | `vec3 m = vec3(dot(u.mat_r.rgb, c), dot(u.mat_g.rgb, c), dot(u.mat_b.rg` | 3 | 6 | 7 |
| 23 | `float lum = dot(m, vec3(0.2126, 0.7152, 0.0722));` | 0 | 0 | 2 |
| 24 | `m = mix(vec3(lum), m, u.saturation);` | 1 | 2 | 4 |
| 25 | `vec3 g = pow(max(m * u.gain.rgb + u.lift.rgb * (1.0 - m), vec3(0.0)), ` | 5 | 8 | 8 |
| 26 | `o = vec4(clamp(g, 0.0, 1.0), 1.0);` | 2 | 0 | 6 |
