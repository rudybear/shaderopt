# bloom_composite: exact canonicalization

Variants produced by `shader-ir rewrite` from the baseline SPIR-V, verified in the CPU model (f64 and f32) against the original on the first train scenario, then measured on the desktop GPU interleaved with the baseline (B,V,B,V...). `exact` ops are bit-identical by IEEE semantics; `ulp` ops are identical in real arithmetic with bounded rounding differences.

## canon_exact

Passes: `fold,dce,cse,ident,unroll,select`. Edit ops: 2 (exact=2, ulp=0); by pass: cse=1, fold=1.

CPU verification:

- composite:f64: bit-identical; max 0 f32 ULP, abs max 0, masked 0
- composite:f32: bit-identical; max 0 f32 ULP, abs max 0, masked 0

GPU measurement (desktop):

| scenario | pass | baseline us | variant us | speedup | 95% CI | gates 1-4 | verdict |
|---|---|---|---|---|---|---|---|
| bloom_hdr_edges | composite | 7.46 | 7.47 | -0.08% | [-0.27%, +0.16%] | True True True False | no measurable effect (driver already does this, or nothing to gain) |
| bloom_hdr_gradient | composite | 7.48 | 7.49 | -0.08% | [-0.27%, +0.21%] | True True True False | no measurable effect (driver already does this, or nothing to gain) |

## canon_full

Passes: `fold,dce,cse,ident,unroll,select,divconst,powspec`. Edit ops: 2 (exact=2, ulp=0); by pass: cse=1, fold=1.

CPU verification:

- composite:f64: bit-identical; max 0 f32 ULP, abs max 0, masked 0
- composite:f32: bit-identical; max 0 f32 ULP, abs max 0, masked 0

GPU measurement (desktop):

| scenario | pass | baseline us | variant us | speedup | 95% CI | gates 1-4 | verdict |
|---|---|---|---|---|---|---|---|
| bloom_hdr_edges | composite | 7.46 | 7.46 | +0.05% | [-0.11%, +0.24%] | True True True False | no measurable effect (driver already does this, or nothing to gain) |
| bloom_hdr_gradient | composite | 7.48 | 7.48 | +0.00% | [-0.13%, +0.24%] | True True True False | no measurable effect (driver already does this, or nothing to gain) |

