# fxaa: exact canonicalization

Variants produced by `shader-ir rewrite` from the baseline SPIR-V, verified in the CPU model (f64 and f32) against the original on the first train scenario, then measured on the desktop GPU interleaved with the baseline (B,V,B,V...). `exact` ops are bit-identical by IEEE semantics; `ulp` ops are identical in real arithmetic with bounded rounding differences.

## canon_exact

Passes: `fold,dce,cse,ident,unroll,select`. Edit ops: 50 (exact=50, ulp=0); by pass: cse=50.

CPU verification:

- fxaa:f64: bit-identical; max 0 f32 ULP, abs max 0, masked 0
- fxaa:f32: bit-identical; max 0 f32 ULP, abs max 0, masked 0

GPU measurement (desktop):

| scenario | pass | baseline us | variant us | speedup | 95% CI | gates 1-4 | verdict |
|---|---|---|---|---|---|---|---|
| fxaa_checker | fxaa | 23.11 | 23.09 | +0.06% | [-0.07%, +0.13%] | True True True False | no measurable effect (driver already does this, or nothing to gain) |
| fxaa_edges | fxaa | 23.74 | 23.76 | -0.08% | [-0.28%, +0.04%] | True True True False | no measurable effect (driver already does this, or nothing to gain) |

## canon_full

Passes: `fold,dce,cse,ident,unroll,select,divconst,powspec`. Edit ops: 50 (exact=50, ulp=0); by pass: cse=50.

CPU verification:

- fxaa:f64: bit-identical; max 0 f32 ULP, abs max 0, masked 0
- fxaa:f32: bit-identical; max 0 f32 ULP, abs max 0, masked 0

GPU measurement (desktop):

| scenario | pass | baseline us | variant us | speedup | 95% CI | gates 1-4 | verdict |
|---|---|---|---|---|---|---|---|
| fxaa_checker | fxaa | 23.10 | 23.09 | +0.03% | [-0.09%, +0.10%] | True True True False | no measurable effect (driver already does this, or nothing to gain) |
| fxaa_edges | fxaa | 23.75 | 23.75 | +0.01% | [-0.12%, +0.13%] | True True True False | no measurable effect (driver already does this, or nothing to gain) |

