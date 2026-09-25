# color_grade: exact canonicalization

Variants produced by `shader-ir rewrite` from the baseline SPIR-V, verified in the CPU model (f64 and f32) against the original on the first train scenario, then measured on the desktop GPU interleaved with the baseline (B,V,B,V...). `exact` ops are bit-identical by IEEE semantics; `ulp` ops are identical in real arithmetic with bounded rounding differences.

## canon_exact

Passes: `fold,dce,cse,ident,unroll,select`. Edit ops: 7 (exact=7, ulp=0); by pass: cse=3, fold=4.

CPU verification:

- grade:f64: bit-identical; max 0 f32 ULP, abs max 0, masked 0
- grade:f32: bit-identical; max 0 f32 ULP, abs max 0, masked 0

GPU measurement (desktop):

| scenario | pass | baseline us | variant us | speedup | 95% CI | gates 1-4 | verdict |
|---|---|---|---|---|---|---|---|
| colorgrade_gradient | grade | 6.85 | 6.85 | +0.06% | [-0.18%, +0.29%] | True True True False | no measurable effect (driver already does this, or nothing to gain) |
| colorgrade_photo | grade | 6.75 | 6.76 | -0.09% | [-0.24%, +0.09%] | True True True False | no measurable effect (driver already does this, or nothing to gain) |

## canon_full

Passes: `fold,dce,cse,ident,unroll,select,divconst,powspec`. Edit ops: 7 (exact=7, ulp=0); by pass: cse=3, fold=4.

CPU verification:

- grade:f64: bit-identical; max 0 f32 ULP, abs max 0, masked 0
- grade:f32: bit-identical; max 0 f32 ULP, abs max 0, masked 0

GPU measurement (desktop):

| scenario | pass | baseline us | variant us | speedup | 95% CI | gates 1-4 | verdict |
|---|---|---|---|---|---|---|---|
| colorgrade_gradient | grade | 6.85 | 6.85 | -0.09% | [-0.23%, +0.12%] | True True True False | no measurable effect (driver already does this, or nothing to gain) |
| colorgrade_photo | grade | 6.76 | 6.75 | +0.06% | [-0.09%, +0.33%] | True True True False | no measurable effect (driver already does this, or nothing to gain) |

