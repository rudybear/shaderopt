# bloom_composite: classification

Scenarios used for ranges: bloom_hdr_edges, bloom_hdr_gradient (train). Sensitivity measured on bloom_hdr_edges / pass composite (RGBA8_SRGB, metric color), each candidate site rounded to f16 in the CPU f32 model. Source: `lab/shaders/bloom_composite.frag`; ids are result ids of `lab/build/spv/bloom_composite.spv`.

## Summary

| | |
|---|---|
| candidate_sites | 23 |
| const | 1 |
| float_sites | 25 |
| pixel | 22 |
| sink_sites | 2 |
| uniform | 4 |

Rate histogram (results): const=9, pixel=23, uniform=4

## Hard sinks (zero budget)

| kind | sites | source lines |
|---|---|---|
| address | 2 | 14, 15 |
| control | 0 |  |
| discard | 0 |  |
| convert | 0 |  |

Branch conditions and sample coordinates by source line:

- L14 `vec3 scene = texture(u_scene, uv).rgb * u.exposure;` -> address
- L15 `vec3 bloom = texture(u_bloom, uv).rgb;` -> address

## Samplers

| sampler | binding | samples | coordinate kinds |
|---|---|---|---|
| u_scene | 0 | 1 | uv_exact |
| u_bloom | 1 | 1 | uv_exact |

## Ranges (from the CPU model over scenario pixels)

Float results with NaN/Inf observed, or ranges beyond f16 (|x| > 65504) or below f16 normal (|x| < 6.1e-5 while nonzero):

- none

## Sensitivity to f16 rounding per site (CPU prediction)

All candidate sites at once: FLIP mean 0.0009, p99 0.0145, max 0.0508.

Most sensitive sites (avoid or budget carefully):

| id | op | rate | line | source | FLIP mean | FLIP p99 |
|---|---|---|---|---|---|---|
| %64 | OpExtInst/Pow | pixel | 17 | `o = vec4(pow(c, vec3(1.0 / 2.2)), 1.0);` | 0.0007 | 0.0120 |
| %68 | OpCompositeConstruct | pixel | 17 | `o = vec4(pow(c, vec3(1.0 / 2.2)), 1.0);` | 0.0007 | 0.0120 |
| %13 | OpLoad | pixel | 12 | `vec3 tonemap(vec3 x) { return x / (1.0 + x); }` | 0.0005 | 0.0099 |
| %66 | OpCompositeExtract | pixel | 17 | `o = vec4(pow(c, vec3(1.0 / 2.2)), 1.0);` | 0.0004 | 0.0086 |
| %17 | OpFAdd | pixel | 12 | `vec3 tonemap(vec3 x) { return x / (1.0 + x); }` | 0.0004 | 0.0080 |
| %58 | OpFunctionCall | pixel | 16 | `vec3 c = tonemap(scene + bloom * u.intensity);` | 0.0003 | 0.0063 |
| %61 | OpLoad | pixel | 17 | `o = vec4(pow(c, vec3(1.0 / 2.2)), 1.0);` | 0.0003 | 0.0063 |
| %18 | OpFDiv | pixel | 12 | `vec3 tonemap(vec3 x) { return x / (1.0 + x); }` | 0.0003 | 0.0063 |
| %65 | OpCompositeExtract | pixel | 17 | `o = vec4(pow(c, vec3(1.0 / 2.2)), 1.0);` | 0.0003 | 0.0062 |
| %15 | OpLoad | pixel | 12 | `vec3 tonemap(vec3 x) { return x / (1.0 + x); }` | 0.0003 | 0.0061 |
| %56 | OpFAdd | pixel | 16 | `vec3 c = tonemap(scene + bloom * u.intensity);` | 0.0002 | 0.0044 |
| %67 | OpCompositeExtract | pixel | 17 | `o = vec4(pow(c, vec3(1.0 / 2.2)), 1.0);` | 0.0003 | 0.0039 |

Sites whose f16 rounding changes no output code at all: 1 of 22 (free demotion candidates): %41

## Pass graph

| scenario | producer | consumer | sampler | coordinate kinds | same resolution | fusable |
|---|---|---|---|---|---|---|
| bloom_extreme | blur_v (gaussian_blur_v) | composite | u_bloom | uv_exact | False | no |
| bloom_hdr_edges | blur_v (gaussian_blur_v) | composite | u_bloom | uv_exact | False | no |
| bloom_hdr_gradient | blur_v (gaussian_blur_v) | composite | u_bloom | uv_exact | False | no |
| bloom_noise | blur_v (gaussian_blur_v) | composite | u_bloom | uv_exact | False | no |

## Rate by source line

| line | source | const | uniform | pixel |
|---|---|---|---|---|
| 12 | `vec3 tonemap(vec3 x) { return x / (1.0 + x); }` | 1 | 0 | 4 |
| 14 | `vec3 scene = texture(u_scene, uv).rgb * u.exposure;` | 1 | 2 | 4 |
| 15 | `vec3 bloom = texture(u_bloom, uv).rgb;` | 0 | 1 | 3 |
| 16 | `vec3 c = tonemap(scene + bloom * u.intensity);` | 1 | 1 | 5 |
| 17 | `o = vec4(pow(c, vec3(1.0 / 2.2)), 1.0);` | 0 | 0 | 6 |
