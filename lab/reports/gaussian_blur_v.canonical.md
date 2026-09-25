# gaussian_blur_v: exact canonicalization

Variants produced by `shader-ir rewrite` from the baseline SPIR-V, verified in the CPU model (f64 and f32) against the original on the first train scenario, then measured on the desktop GPU interleaved with the baseline (B,V,B,V...). `exact` ops are bit-identical by IEEE semantics; `ulp` ops are identical in real arithmetic with bounded rounding differences.

## canon_exact

Passes: `fold,dce,cse,ident,unroll,select`. Edit ops: 78 (exact=78, ulp=0); by pass: cse=58, dce=1, fold=18, unroll=1.

CPU verification:

- blur_v:f64: bit-identical; max 0 f32 ULP, abs max 0, masked 0
- blur_v:f32: bit-identical; max 0 f32 ULP, abs max 0, masked 0

GPU measurement (desktop):

| scenario | pass | baseline us | variant us | speedup | 95% CI | gates 1-4 | verdict |
|---|---|---|---|---|---|---|---|
| bloom_hdr_edges | blur_v | 4.96 | 4.96 | +0.16% | [-0.08%, +0.36%] | True True True False | no measurable effect (driver already does this, or nothing to gain) |
| bloom_hdr_gradient | blur_v | 4.96 | 4.96 | +0.00% | [-0.16%, +0.20%] | True True True False | no measurable effect (driver already does this, or nothing to gain) |

## canon_full

Passes: `fold,dce,cse,ident,unroll,select,divconst,powspec`. Edit ops: 78 (exact=78, ulp=0); by pass: cse=58, dce=1, fold=18, unroll=1.

CPU verification:

- blur_v:f64: bit-identical; max 0 f32 ULP, abs max 0, masked 0
- blur_v:f32: bit-identical; max 0 f32 ULP, abs max 0, masked 0

GPU measurement (desktop):

| scenario | pass | baseline us | variant us | speedup | 95% CI | gates 1-4 | verdict |
|---|---|---|---|---|---|---|---|
| bloom_hdr_edges | blur_v | 4.96 | 4.96 | -0.04% | [-0.24%, +0.24%] | True True True False | no measurable effect (driver already does this, or nothing to gain) |
| bloom_hdr_gradient | blur_v | 4.96 | 4.96 | +0.00% | [-0.20%, +0.24%] | True True True False | no measurable effect (driver already does this, or nothing to gain) |

