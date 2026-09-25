# deferred_lit: exact canonicalization

Variants produced by `shader-ir rewrite` from the baseline SPIR-V, verified in the CPU model (f64 and f32) against the original on the first train scenario, then measured on the desktop GPU interleaved with the baseline (B,V,B,V...). `exact` ops are bit-identical by IEEE semantics; `ulp` ops are identical in real arithmetic with bounded rounding differences.

## canon_exact

Passes: `fold,dce,cse,ident,unroll,select`. Edit ops: 98 (exact=98, ulp=0); by pass: cse=51, dce=2, fold=40, ident=1, select=2, unroll=2.

CPU verification:

- lit:f64: bit-identical; max 0 f32 ULP, abs max 0, masked 1079132
- lit:f32: bit-identical; max 0 f32 ULP, abs max 0, masked 1079132

GPU measurement (desktop):

| scenario | pass | baseline us | variant us | speedup | 95% CI | gates 1-4 | verdict |
|---|---|---|---|---|---|---|---|
| deferred_spheres | lit | 30.79 | 30.82 | -0.08% | [-0.24%, +0.03%] | True True True False | no measurable effect (driver already does this, or nothing to gain) |

## canon_full

Passes: `fold,dce,cse,ident,unroll,select,divconst,powspec`. Edit ops: 99 (exact=98, ulp=1); by pass: cse=51, dce=2, divconst=1, fold=40, ident=1, select=2, unroll=2.

CPU verification:

- lit:f64: DIFFERS; max 1 f32 ULP, abs max 5.96e-08, masked 1079132
- lit:f32: bit-identical; max 0 f32 ULP, abs max 0, masked 1079132

GPU measurement (desktop):

| scenario | pass | baseline us | variant us | speedup | 95% CI | gates 1-4 | verdict |
|---|---|---|---|---|---|---|---|
| deferred_spheres | lit | 30.79 | 30.82 | -0.08% | [-0.20%, +0.05%] | True True True False | no measurable effect (driver already does this, or nothing to gain) |

