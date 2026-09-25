# deferred_lit: classification

Scenarios used for ranges: deferred_spheres (train). Sensitivity measured on deferred_spheres / pass lit (RGBA8, metric color), each candidate site rounded to f16 in the CPU f32 model. Source: `lab/shaders/deferred_lit.frag`; ids are result ids of `lab/build/spv/deferred_lit.spv`.

## Summary

| | |
|---|---|
| candidate_sites | 48 |
| const | 14 |
| float_sites | 74 |
| pixel | 64 |
| sink_sites | 44 |
| uniform | 18 |

Rate histogram (results): const=59, pixel=66, uniform=18

## Hard sinks (zero budget)

| kind | sites | source lines |
|---|---|---|
| address | 22 | 20, 21, 22, 28, 29, 34, 36 |
| control | 33 | 20, 21, 22, 23, 28, 29, 30, 34, 36 |
| discard | 8 | 28, 29, 30 |
| convert | 0 |  |

Branch conditions and sample coordinates by source line:

- L28 `vec4 alb = texture(u_albedo, uv);` -> address
- L29 `vec4 nrm = texture(u_normal, uv);` -> address
- L30 `if (alb.a < 0.5 || nrm.w < 0.5) discard;` -> control, discard
- L34 `vec3 suv = texture(u_shadowuv, uv).xyz;` -> address
- L36 `if (suv.z > 0.0 && suv.z < 1.0) sh = mix(u.shadow_min, 1.0, pcf3(suv));` -> control
- L20 `for (int v = -1; v <= 1; v++)` -> control
- L21 `for (int h = -1; h <= 1; h++) {` -> control
- L22 `float d = texture(u_shadow, uvw.xy + u.shadow_texel * vec2(h, v)).r;` -> address
- L23 `s += (uvw.z + u.depth_bias <= d) ? 1.0 : 0.0;` -> control

## Samplers

| sampler | binding | samples | coordinate kinds |
|---|---|---|---|
| u_shadow | 3 | 1 | other |
| u_albedo | 0 | 1 | uv_exact |
| u_normal | 1 | 1 | uv_exact |
| u_shadowuv | 2 | 1 | uv_exact |

## Ranges (from the CPU model over scenario pixels)

Float results with NaN/Inf observed, or ranges beyond f16 (|x| > 65504) or below f16 normal (|x| < 6.1e-5 while nonzero):

- %71 OpLoad L23 `s += (uvw.z + u.depth_bias <= d) ? 1.0 : 0.0;`: [-5e-05, -5e-05] nan=0 inf=0

## Sensitivity to f16 rounding per site (CPU prediction)

All candidate sites at once: FLIP mean 0.0005, p99 0.0084, max 0.0247.

Most sensitive sites (avoid or budget carefully):

| id | op | rate | line | source | FLIP mean | FLIP p99 |
|---|---|---|---|---|---|---|
| %131 | OpExtInst/Normalize | uniform | 32 | `float ndl1 = clamp(dot(n, normalize(u.light1.xyz)), 0.0, 1.0` | 0.0002 | 0.0054 |
| %174 | OpLoad | pixel | 37 | `vec3 c = alb.rgb * (u.ambient.rgb + vec3(0.5 * (ndl1 + ndl2)` | 0.0003 | 0.0053 |
| %175 | OpVectorShuffle | pixel | 37 | `vec3 c = alb.rgb * (u.ambient.rgb + vec3(0.5 * (ndl1 + ndl2)` | 0.0003 | 0.0053 |
| %188 | OpFMul | pixel | 37 | `vec3 c = alb.rgb * (u.ambient.rgb + vec3(0.5 * (ndl1 + ndl2)` | 0.0002 | 0.0052 |
| %191 | OpLoad | pixel | 38 | `o = vec4(c, 1.0);` | 0.0002 | 0.0052 |
| %195 | OpCompositeConstruct | pixel | 38 | `o = vec4(c, 1.0);` | 0.0002 | 0.0052 |
| %187 | OpFAdd | pixel | 37 | `vec3 c = alb.rgb * (u.ambient.rgb + vec3(0.5 * (ndl1 + ndl2)` | 0.0002 | 0.0051 |
| %193 | OpCompositeExtract | pixel | 38 | `o = vec4(c, 1.0);` | 0.0001 | 0.0045 |
| %117 | OpLoad | pixel | 31 | `vec3 n = normalize(nrm.xyz * 2.0 - 1.0);` | 0.0002 | 0.0045 |
| %118 | OpVectorShuffle | pixel | 31 | `vec3 n = normalize(nrm.xyz * 2.0 - 1.0);` | 0.0002 | 0.0045 |
| %120 | OpVectorTimesScalar | pixel | 31 | `vec3 n = normalize(nrm.xyz * 2.0 - 1.0);` | 0.0002 | 0.0045 |
| %135 | OpLoad | uniform | 32 | `float ndl1 = clamp(dot(n, normalize(u.light1.xyz)), 0.0, 1.0` | 0.0002 | 0.0042 |

Sites whose f16 rounding changes no output code at all: 10 of 47 (free demotion candidates): %129, %130, %140, %141, %171, %76, %77, %78, %83, %85

## Pass graph

This shader never consumes another pass's output in the current scenarios.

## Rate by source line

| line | source | const | uniform | pixel |
|---|---|---|---|---|
| 20 | `for (int v = -1; v <= 1; v++)` | 8 | 0 | 0 |
| 21 | `for (int h = -1; h <= 1; h++) {` | 9 | 0 | 0 |
| 22 | `float d = texture(u_shadow, uvw.xy + u.shadow_texel * vec2(h, v)).r;` | 6 | 3 | 5 |
| 23 | `s += (uvw.z + u.depth_bias <= d) ? 1.0 : 0.0;` | 2 | 1 | 8 |
| 25 | `return s / 9.0;` | 0 | 0 | 2 |
| 28 | `vec4 alb = texture(u_albedo, uv);` | 0 | 1 | 2 |
| 29 | `vec4 nrm = texture(u_normal, uv);` | 0 | 1 | 2 |
| 30 | `if (alb.a < 0.5 || nrm.w < 0.5) discard;` | 6 | 0 | 6 |
| 31 | `vec3 n = normalize(nrm.xyz * 2.0 - 1.0);` | 1 | 0 | 5 |
| 32 | `float ndl1 = clamp(dot(n, normalize(u.light1.xyz)), 0.0, 1.0) * u.ligh` | 2 | 4 | 4 |
| 33 | `float ndl2 = clamp(dot(n, normalize(u.light2.xyz)), 0.0, 1.0) * u.ligh` | 2 | 4 | 4 |
| 34 | `vec3 suv = texture(u_shadowuv, uv).xyz;` | 0 | 1 | 3 |
| 36 | `if (suv.z > 0.0 && suv.z < 1.0) sh = mix(u.shadow_min, 1.0, pcf3(suv))` | 7 | 1 | 8 |
| 37 | `vec3 c = alb.rgb * (u.ambient.rgb + vec3(0.5 * (ndl1 + ndl2)) * sh);` | 1 | 2 | 11 |
| 38 | `o = vec4(c, 1.0);` | 0 | 0 | 5 |
