# bloom_threshold: exact canonicalization

Variants produced by `shader-ir rewrite` from the baseline SPIR-V, verified in the CPU model (f64 and f32) against the original on the first train scenario, then measured on the desktop GPU interleaved with the baseline (B,V,B,V...). `exact` ops are bit-identical by IEEE semantics; `ulp` ops are identical in real arithmetic with bounded rounding differences.

## canon_exact

Passes: `fold,dce,cse,ident,unroll,select`. Edit ops: 4 (exact=4, ulp=0); by pass: cse=4.

CPU verification:

- threshold:f64: bit-identical; max 0 f32 ULP, abs max 0, masked 0
- threshold:f32: bit-identical; max 0 f32 ULP, abs max 0, masked 0

GPU measurement (desktop):

| scenario | pass | baseline us | variant us | speedup | 95% CI | gates 1-4 | verdict |
|---|---|---|---|---|---|---|---|
| bloom_hdr_edges | threshold | 6.48 | 6.48 | +0.00% | [-0.19%, +0.15%] | True True True False | no measurable effect (driver already does this, or nothing to gain) |
| bloom_hdr_gradient | threshold | 6.50 | 6.50 | +0.09% | [-0.18%, +0.25%] | True True True False | no measurable effect (driver already does this, or nothing to gain) |

## canon_full

Passes: `fold,dce,cse,ident,unroll,select,divconst,powspec`. Edit ops: 4 (exact=4, ulp=0); by pass: cse=4.

CPU verification:

- threshold:f64: bit-identical; max 0 f32 ULP, abs max 0, masked 0
- threshold:f32: bit-identical; max 0 f32 ULP, abs max 0, masked 0

GPU measurement (desktop):

| scenario | pass | baseline us | variant us | speedup | 95% CI | gates 1-4 | verdict |
|---|---|---|---|---|---|---|---|
| bloom_hdr_edges | threshold | 6.48 | 6.49 | -0.19% | [-0.40%, +0.06%] | True True True False | no measurable effect (driver already does this, or nothing to gain) |
| bloom_hdr_gradient | threshold | 6.52 | 6.52 | -0.12% | [-0.25%, +0.12%] | True True True False | no measurable effect (driver already does this, or nothing to gain) |

