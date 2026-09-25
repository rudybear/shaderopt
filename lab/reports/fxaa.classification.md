# fxaa: classification

Scenarios used for ranges: fxaa_checker, fxaa_edges (train). Sensitivity measured on fxaa_checker / pass fxaa (RGBA8, metric color), each candidate site rounded to f16 in the CPU f32 model. Source: `lab/shaders/fxaa.frag`; ids are result ids of `lab/build/spv/fxaa.spv`.

## Summary

| | |
|---|---|
| candidate_sites | 40 |
| const | 0 |
| float_sites | 215 |
| pixel | 190 |
| sink_sites | 180 |
| uniform | 43 |

Rate histogram (results): const=57, pixel=191, uniform=43

## Hard sinks (zero budget)

| kind | sites | source lines |
|---|---|---|
| address | 149 | 14, 16, 17, 18, 19, 20, 21, 26, 27, 28, 29, 35, 36, 37, 38, 39, 40, 41, 42 |
| control | 90 | 14, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 40, 41, 42, 43 |
| discard | 0 |  |
| convert | 0 |  |

Branch conditions and sample coordinates by source line:

- L16 `vec3 rgbM = texture(u_src, uv).rgb;` -> address
- L17 `float lM = luma(rgbM);` -> address, control
- L18 `float lN = luma(texture(u_src, uv + vec2(0.0, -u.texel.y)).rgb);` -> address
- L19 `float lS = luma(texture(u_src, uv + vec2(0.0,  u.texel.y)).rgb);` -> address
- L20 `float lW = luma(texture(u_src, uv + vec2(-u.texel.x, 0.0)).rgb);` -> address
- L21 `float lE = luma(texture(u_src, uv + vec2( u.texel.x, 0.0)).rgb);` -> address
- L22 `float lMin = min(lM, min(min(lN, lS), min(lW, lE)));` -> control
- L23 `float lMax = max(lM, max(max(lN, lS), max(lW, lE)));` -> control
- L24 `float range = lMax - lMin;` -> control
- L25 `if (range < max(u.edge_threshold_min, lMax * u.edge_threshold)) { o = vec4(rgbM, 1.0); return; }` -> control
- L26 `float lNW = luma(texture(u_src, uv + vec2(-u.texel.x, -u.texel.y)).rgb);` -> address
- L27 `float lNE = luma(texture(u_src, uv + vec2( u.texel.x, -u.texel.y)).rgb);` -> address
- L28 `float lSW = luma(texture(u_src, uv + vec2(-u.texel.x,  u.texel.y)).rgb);` -> address
- L29 `float lSE = luma(texture(u_src, uv + vec2( u.texel.x,  u.texel.y)).rgb);` -> address
- L35 `dir.x = -((lNW + lNE) - (lSW + lSE));` -> address
- L36 `dir.y =  ((lNW + lSW) - (lNE + lSE));` -> address
- L37 `float dirReduce = max((lNW + lNE + lSW + lSE) * 0.03125, 0.0078125);` -> address
- L38 `float rcpDirMin = 1.0 / (min(abs(dir.x), abs(dir.y)) + dirReduce);` -> address
- L39 `dir = clamp(dir * rcpDirMin, vec2(-8.0), vec2(8.0)) * u.texel;` -> address
- L40 `vec3 rgbA = 0.5 * (texture(u_src, uv + dir * (1.0 / 3.0 - 0.5)).rgb + texture(u_src, uv + dir * (2.0 / 3.0 - 0.5)).rgb);` -> address
- L41 `vec3 rgbB = rgbA * 0.5 + 0.25 * (texture(u_src, uv + dir * -0.5).rgb + texture(u_src, uv + dir * 0.5).rgb);` -> address, control
- L42 `float lB = luma(rgbB);` -> address, control
- L43 `vec3 result = (lB < lMin || lB > lMax) ? rgbA : rgbB;` -> control
- L14 `float luma(vec3 c) { return dot(c, vec3(0.299, 0.587, 0.114)); }` -> address, control

## Samplers

| sampler | binding | samples | coordinate kinds |
|---|---|---|---|
| u_src | 0 | 13 | other, uv_exact, uv_offset |

## Ranges (from the CPU model over scenario pixels)

Float results with NaN/Inf observed, or ranges beyond f16 (|x| > 65504) or below f16 normal (|x| < 6.1e-5 while nonzero):

- none

## Sensitivity to f16 rounding per site (CPU prediction)

All candidate sites at once: FLIP mean 0.0000, p99 0.0000, max 0.0000.

Most sensitive sites (avoid or budget carefully):

| id | op | rate | line | source | FLIP mean | FLIP p99 |
|---|---|---|---|---|---|---|
| %135 | OpLoad | pixel | 25 | `if (range < max(u.edge_threshold_min, lMax * u.edge_threshol` | 0.0000 | 0.0000 |
| %137 | OpCompositeExtract | pixel | 25 | `if (range < max(u.edge_threshold_min, lMax * u.edge_threshol` | 0.0000 | 0.0000 |
| %138 | OpCompositeExtract | pixel | 25 | `if (range < max(u.edge_threshold_min, lMax * u.edge_threshol` | 0.0000 | 0.0000 |
| %139 | OpCompositeExtract | pixel | 25 | `if (range < max(u.edge_threshold_min, lMax * u.edge_threshol` | 0.0000 | 0.0000 |
| %140 | OpCompositeConstruct | pixel | 25 | `if (range < max(u.edge_threshold_min, lMax * u.edge_threshol` | 0.0000 | 0.0000 |
| %200 | OpLoad | pixel | 30 | `float lumaL = 0.25 * (lN + lS + lW + lE);` | 0.0000 | 0.0000 |
| %201 | OpLoad | pixel | 30 | `float lumaL = 0.25 * (lN + lS + lW + lE);` | 0.0000 | 0.0000 |
| %202 | OpFAdd | pixel | 30 | `float lumaL = 0.25 * (lN + lS + lW + lE);` | 0.0000 | 0.0000 |
| %203 | OpLoad | pixel | 30 | `float lumaL = 0.25 * (lN + lS + lW + lE);` | 0.0000 | 0.0000 |
| %204 | OpFAdd | pixel | 30 | `float lumaL = 0.25 * (lN + lS + lW + lE);` | 0.0000 | 0.0000 |
| %205 | OpLoad | pixel | 30 | `float lumaL = 0.25 * (lN + lS + lW + lE);` | 0.0000 | 0.0000 |
| %206 | OpFAdd | pixel | 30 | `float lumaL = 0.25 * (lN + lS + lW + lE);` | 0.0000 | 0.0000 |

Sites whose f16 rounding changes no output code at all: 40 of 40 (free demotion candidates): %135, %137, %138, %139, %140, %200, %201, %202, %203, %204, %205, %206, %207, %209, %210, %211, %212, %214, %215, %216, %217, %219, %220, %221, %222, %223, %226, %227, %335, %336, %339, %340, %341, %342, %343, %344, %345, %346, %347, %348

## Pass graph

This shader never consumes another pass's output in the current scenarios.

## Rate by source line

| line | source | const | uniform | pixel |
|---|---|---|---|---|
| 14 | `float luma(vec3 c) { return dot(c, vec3(0.299, 0.587, 0.114)); }` | 0 | 0 | 2 |
| 16 | `vec3 rgbM = texture(u_src, uv).rgb;` | 0 | 1 | 3 |
| 17 | `float lM = luma(rgbM);` | 0 | 0 | 2 |
| 18 | `float lN = luma(texture(u_src, uv + vec2(0.0, -u.texel.y)).rgb);` | 1 | 4 | 5 |
| 19 | `float lS = luma(texture(u_src, uv + vec2(0.0,  u.texel.y)).rgb);` | 1 | 3 | 5 |
| 20 | `float lW = luma(texture(u_src, uv + vec2(-u.texel.x, 0.0)).rgb);` | 1 | 4 | 5 |
| 21 | `float lE = luma(texture(u_src, uv + vec2( u.texel.x, 0.0)).rgb);` | 1 | 3 | 5 |
| 22 | `float lMin = min(lM, min(min(lN, lS), min(lW, lE)));` | 0 | 0 | 9 |
| 23 | `float lMax = max(lM, max(max(lN, lS), max(lW, lE)));` | 0 | 0 | 9 |
| 24 | `float range = lMax - lMin;` | 0 | 0 | 3 |
| 25 | `if (range < max(u.edge_threshold_min, lMax * u.edge_threshold)) { o = ` | 4 | 2 | 10 |
| 26 | `float lNW = luma(texture(u_src, uv + vec2(-u.texel.x, -u.texel.y)).rgb` | 2 | 6 | 5 |
| 27 | `float lNE = luma(texture(u_src, uv + vec2( u.texel.x, -u.texel.y)).rgb` | 2 | 5 | 5 |
| 28 | `float lSW = luma(texture(u_src, uv + vec2(-u.texel.x,  u.texel.y)).rgb` | 2 | 5 | 5 |
| 29 | `float lSE = luma(texture(u_src, uv + vec2( u.texel.x,  u.texel.y)).rgb` | 2 | 4 | 5 |
| 30 | `float lumaL = 0.25 * (lN + lS + lW + lE);` | 0 | 0 | 8 |
| 31 | `float rangeL = abs(lumaL - lM);` | 0 | 0 | 4 |
| 32 | `float blendL = clamp((rangeL / range - 0.25) * 4.0, 0.0, 1.0);` | 0 | 0 | 6 |
| 33 | `blendL = blendL * blendL * u.subpix;` | 1 | 1 | 4 |
| 35 | `dir.x = -((lNW + lNE) - (lSW + lSE));` | 1 | 0 | 8 |
| 36 | `dir.y =  ((lNW + lSW) - (lNE + lSE));` | 1 | 0 | 7 |
| 37 | `float dirReduce = max((lNW + lNE + lSW + lSE) * 0.03125, 0.0078125);` | 0 | 0 | 9 |
| 38 | `float rcpDirMin = 1.0 / (min(abs(dir.x), abs(dir.y)) + dirReduce);` | 2 | 0 | 8 |
| 39 | `dir = clamp(dir * rcpDirMin, vec2(-8.0), vec2(8.0)) * u.texel;` | 1 | 1 | 5 |
| 40 | `vec3 rgbA = 0.5 * (texture(u_src, uv + dir * (1.0 / 3.0 - 0.5)).rgb + ` | 0 | 2 | 14 |
| 41 | `vec3 rgbB = rgbA * 0.5 + 0.25 * (texture(u_src, uv + dir * -0.5).rgb +` | 0 | 2 | 17 |
| 42 | `float lB = luma(rgbB);` | 0 | 0 | 2 |
| 43 | `vec3 result = (lB < lMin || lB > lMax) ? rgbA : rgbB;` | 0 | 0 | 11 |
| 44 | `o = vec4(mix(rgbM, result, blendL), 1.0);` | 0 | 0 | 9 |
