# gaussian_blur_v: classification

Scenarios used for ranges: bloom_hdr_edges, bloom_hdr_gradient (train). Sensitivity measured on bloom_hdr_edges / pass blur_v (RGBA16F, metric hdr), each candidate site rounded to f16 in the CPU f32 model. Source: `lab/shaders/gaussian_blur_v.frag`; ids are result ids of `lab/build/spv/gaussian_blur_v.spv`.

## Summary

| | |
|---|---|
| candidate_sites | 26 |
| const | 11 |
| float_sites | 33 |
| pixel | 13 |
| sink_sites | 12 |
| uniform | 15 |

Rate histogram (results): const=25, pixel=13, uniform=15

## Hard sinks (zero budget)

| kind | sites | source lines |
|---|---|---|
| address | 10 | 14, 15, 17 |
| control | 4 | 14 |
| discard | 0 |  |
| convert | 0 |  |

Branch conditions and sample coordinates by source line:

- L14 `for (int i = -4; i <= 4; ++i) {` -> control
- L15 `float x = float(i);` -> address
- L17 `acc += texture(u_src, uv + vec2(0.0, x * u.texel.y)).rgb * w;` -> address

## Samplers

| sampler | binding | samples | coordinate kinds |
|---|---|---|---|
| u_src | 0 | 1 | uv_offset |

## Ranges (from the CPU model over scenario pixels)

Float results with NaN/Inf observed, or ranges beyond f16 (|x| > 65504) or below f16 normal (|x| < 6.1e-5 while nonzero):

- none

## Sensitivity to f16 rounding per site (CPU prediction)

All candidate sites at once: FLIP mean 0.0006, p99 0.0031, max 0.0061.

Most sensitive sites (avoid or budget carefully):

| id | op | rate | line | source | FLIP mean | FLIP p99 |
|---|---|---|---|---|---|---|
| %71 | OpLoad | pixel | 17 | `acc += texture(u_src, uv + vec2(0.0, x * u.texel.y)).rgb * w` | 0.0007 | 0.0038 |
| %74 | OpLoad | uniform | 18 | `wsum += w;` | 0.0004 | 0.0029 |
| %48 | OpExtInst/Exp | uniform | 16 | `float w = exp(-0.5 * x * x / (u.sigma * u.sigma));` | 0.0003 | 0.0026 |
| %72 | OpFAdd | pixel | 17 | `acc += texture(u_src, uv + vec2(0.0, x * u.texel.y)).rgb * w` | 0.0005 | 0.0025 |
| %69 | OpLoad | uniform | 17 | `acc += texture(u_src, uv + vec2(0.0, x * u.texel.y)).rgb * w` | 0.0003 | 0.0024 |
| %80 | OpLoad | pixel | 20 | `o = vec4(acc / wsum, 1.0);` | 0.0003 | 0.0020 |
| %75 | OpFAdd | uniform | 18 | `wsum += w;` | 0.0002 | 0.0020 |
| %81 | OpLoad | uniform | 20 | `o = vec4(acc / wsum, 1.0);` | 0.0002 | 0.0020 |
| %82 | OpCompositeConstruct | uniform | 20 | `o = vec4(acc / wsum, 1.0);` | 0.0002 | 0.0020 |
| %70 | OpVectorTimesScalar | pixel | 17 | `acc += texture(u_src, uv + vec2(0.0, x * u.texel.y)).rgb * w` | 0.0002 | 0.0019 |
| %73 | OpLoad | uniform | 18 | `wsum += w;` | 0.0002 | 0.0018 |
| %43 | OpLoad | uniform | 16 | `float w = exp(-0.5 * x * x / (u.sigma * u.sigma));` | 0.0000 | 0.0000 |

Sites whose f16 rounding changes no output code at all: 11 of 22 (free demotion candidates): %43, %45, %46, %47, %67, %68, %83, %85, %86, %87, %88

## Pass graph

| scenario | producer | consumer | sampler | coordinate kinds | same resolution | fusable |
|---|---|---|---|---|---|---|
| bloom_extreme | blur_h (gaussian_blur_h) | blur_v | u_src | uv_offset | True | no |
| bloom_hdr_edges | blur_h (gaussian_blur_h) | blur_v | u_src | uv_offset | True | no |
| bloom_hdr_gradient | blur_h (gaussian_blur_h) | blur_v | u_src | uv_offset | True | no |
| bloom_noise | blur_h (gaussian_blur_h) | blur_v | u_src | uv_offset | True | no |

## Rate by source line

| line | source | const | uniform | pixel |
|---|---|---|---|---|
| 14 | `for (int i = -4; i <= 4; ++i) {` | 8 | 0 | 0 |
| 15 | `float x = float(i);` | 2 | 0 | 0 |
| 16 | `float w = exp(-0.5 * x * x / (u.sigma * u.sigma));` | 6 | 5 | 0 |
| 17 | `acc += texture(u_src, uv + vec2(0.0, x * u.texel.y)).rgb * w;` | 2 | 5 | 7 |
| 18 | `wsum += w;` | 1 | 3 | 0 |
| 20 | `o = vec4(acc / wsum, 1.0);` | 0 | 2 | 6 |
