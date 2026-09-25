# bloom_threshold: classification

Scenarios used for ranges: bloom_hdr_edges, bloom_hdr_gradient (train). Sensitivity measured on bloom_hdr_edges / pass threshold (RGBA16F, metric hdr), each candidate site rounded to f16 in the CPU f32 model. Source: `lab/shaders/bloom_threshold.frag`; ids are result ids of `lab/build/spv/bloom_threshold.spv`.

## Summary

| | |
|---|---|
| candidate_sites | 39 |
| const | 0 |
| float_sites | 40 |
| pixel | 28 |
| sink_sites | 1 |
| uniform | 13 |

Rate histogram (results): const=11, pixel=28, uniform=13

## Hard sinks (zero budget)

| kind | sites | source lines |
|---|---|---|
| address | 1 | 13 |
| control | 0 |  |
| discard | 0 |  |
| convert | 0 |  |

Branch conditions and sample coordinates by source line:

- L13 `vec3 c = texture(u_scene, uv).rgb * u.exposure;` -> address

## Samplers

| sampler | binding | samples | coordinate kinds |
|---|---|---|---|
| u_scene | 0 | 1 | uv_exact |

## Ranges (from the CPU model over scenario pixels)

Float results with NaN/Inf observed, or ranges beyond f16 (|x| > 65504) or below f16 normal (|x| < 6.1e-5 while nonzero):

- none

## Sensitivity to f16 rounding per site (CPU prediction)

All candidate sites at once: FLIP mean 0.0008, p99 0.0052, max 0.0061.

Most sensitive sites (avoid or budget carefully):

| id | op | rate | line | source | FLIP mean | FLIP p99 |
|---|---|---|---|---|---|---|
| %77 | OpFDiv | pixel | 18 | `float contrib = max(soft, lum - u.threshold) / max(lum, 1e-5` | 0.0007 | 0.0052 |
| %81 | OpLoad | pixel | 19 | `o = vec4(c * contrib, 1.0);` | 0.0007 | 0.0052 |
| %75 | OpLoad | pixel | 18 | `float contrib = max(soft, lum - u.threshold) / max(lum, 1e-5` | 0.0007 | 0.0052 |
| %76 | OpExtInst/FMax | pixel | 18 | `float contrib = max(soft, lum - u.threshold) / max(lum, 1e-5` | 0.0007 | 0.0052 |
| %70 | OpLoad | pixel | 18 | `float contrib = max(soft, lum - u.threshold) / max(lum, 1e-5` | 0.0006 | 0.0039 |
| %73 | OpFSub | pixel | 18 | `float contrib = max(soft, lum - u.threshold) / max(lum, 1e-5` | 0.0005 | 0.0039 |
| %74 | OpExtInst/FMax | pixel | 18 | `float contrib = max(soft, lum - u.threshold) / max(lum, 1e-5` | 0.0005 | 0.0039 |
| %38 | OpDot | pixel | 14 | `float lum = dot(c, vec3(0.2126, 0.7152, 0.0722));` | 0.0005 | 0.0038 |
| %20 | OpImageSampleImplicitLod | pixel | 13 | `vec3 c = texture(u_scene, uv).rgb * u.exposure;` | 0.0003 | 0.0032 |
| %21 | OpVectorShuffle | pixel | 13 | `vec3 c = texture(u_scene, uv).rgb * u.exposure;` | 0.0003 | 0.0032 |
| %30 | OpVectorTimesScalar | pixel | 13 | `vec3 c = texture(u_scene, uv).rgb * u.exposure;` | 0.0003 | 0.0032 |
| %33 | OpLoad | pixel | 14 | `float lum = dot(c, vec3(0.2126, 0.7152, 0.0722));` | 0.0002 | 0.0029 |

Sites whose f16 rounding changes no output code at all: 20 of 39 (free demotion candidates): %29, %42, %45, %46, %50, %52, %56, %57, %61, %63, %64, %66, %67, %69, %72, %82, %84, %85, %86, %87

## Pass graph

This shader never consumes another pass's output in the current scenarios.

## Rate by source line

| line | source | const | uniform | pixel |
|---|---|---|---|---|
| 13 | `vec3 c = texture(u_scene, uv).rgb * u.exposure;` | 1 | 2 | 4 |
| 14 | `float lum = dot(c, vec3(0.2126, 0.7152, 0.0722));` | 0 | 0 | 2 |
| 15 | `float k = u.knee * u.threshold;` | 2 | 3 | 0 |
| 16 | `float soft = clamp(lum - u.threshold + k, 0.0, 2.0 * k);` | 1 | 4 | 4 |
| 17 | `soft = soft * soft / (4.0 * k + 1e-5);` | 0 | 3 | 4 |
| 18 | `float contrib = max(soft, lum - u.threshold) / max(lum, 1e-5);` | 1 | 1 | 7 |
| 19 | `o = vec4(c * contrib, 1.0);` | 0 | 0 | 7 |
