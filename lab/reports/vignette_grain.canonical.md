# vignette_grain: exact canonicalization

Variants produced by `shader-ir rewrite` from the baseline SPIR-V, verified in the CPU model (f64 and f32) against the original on the first train scenario, then measured on the desktop GPU interleaved with the baseline (B,V,B,V...). `exact` ops are bit-identical by IEEE semantics; `ulp` ops are identical in real arithmetic with bounded rounding differences.

## canon_exact

Passes: `fold,dce,cse,ident,unroll,select`. Edit ops: 7 (exact=7, ulp=0); by pass: cse=4, fold=3.

CPU verification:

- vignette:f64: bit-identical; max 0 f32 ULP, abs max 0, masked 0
- vignette:f32: bit-identical; max 0 f32 ULP, abs max 0, masked 0

GPU measurement (desktop):

| scenario | pass | baseline us | variant us | speedup | 95% CI | gates 1-4 | verdict |
|---|---|---|---|---|---|---|---|
| vignette_gradient | vignette | 6.72 | 6.72 | +0.00% | [-0.21%, +0.18%] | True True True False | no measurable effect (driver already does this, or nothing to gain) |

## canon_full

Passes: `fold,dce,cse,ident,unroll,select,divconst,powspec`. Edit ops: 7 (exact=7, ulp=0); by pass: cse=4, fold=3.

CPU verification:

- vignette:f64: bit-identical; max 0 f32 ULP, abs max 0, masked 0
- vignette:f32: bit-identical; max 0 f32 ULP, abs max 0, masked 0

GPU measurement (desktop):

| scenario | pass | baseline us | variant us | speedup | 95% CI | gates 1-4 | verdict |
|---|---|---|---|---|---|---|---|
| vignette_gradient | vignette | 6.72 | 6.72 | -0.09% | [-0.27%, +0.09%] | True True True False | no measurable effect (driver already does this, or nothing to gain) |

