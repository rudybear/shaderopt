# tonemap_aces: exact canonicalization

Variants produced by `shader-ir rewrite` from the baseline SPIR-V, verified in the CPU model (f64 and f32) against the original on the first train scenario, then measured on the desktop GPU interleaved with the baseline (B,V,B,V...). `exact` ops are bit-identical by IEEE semantics; `ulp` ops are identical in real arithmetic with bounded rounding differences.

## canon_exact

Passes: `fold,dce,cse,ident,unroll,select`. Edit ops: 6 (exact=6, ulp=0); by pass: fold=6.

CPU verification:

- tonemap:f64: bit-identical; max 0 f32 ULP, abs max 0, masked 0
- tonemap:f32: bit-identical; max 0 f32 ULP, abs max 0, masked 0

GPU measurement (desktop):

| scenario | pass | baseline us | variant us | speedup | 95% CI | gates 1-4 | verdict |
|---|---|---|---|---|---|---|---|
| tonemap_hdr_edges | tonemap | 7.71 | 7.70 | +0.08% | [-0.18%, +0.26%] | True True True False | no measurable effect (driver already does this, or nothing to gain) |
| tonemap_hdr_gradient | tonemap | 7.84 | 7.84 | +0.03% | [-0.15%, +0.25%] | True True True False | no measurable effect (driver already does this, or nothing to gain) |

## canon_full

Passes: `fold,dce,cse,ident,unroll,select,divconst,powspec`. Edit ops: 6 (exact=6, ulp=0); by pass: fold=6.

CPU verification:

- tonemap:f64: bit-identical; max 0 f32 ULP, abs max 0, masked 0
- tonemap:f32: bit-identical; max 0 f32 ULP, abs max 0, masked 0

GPU measurement (desktop):

| scenario | pass | baseline us | variant us | speedup | 95% CI | gates 1-4 | verdict |
|---|---|---|---|---|---|---|---|
| tonemap_hdr_edges | tonemap | 7.71 | 7.71 | +0.00% | [-0.23%, +0.18%] | True True True False | no measurable effect (driver already does this, or nothing to gain) |
| tonemap_hdr_gradient | tonemap | 7.84 | 7.84 | +0.03% | [-0.23%, +0.18%] | True True True False | no measurable effect (driver already does this, or nothing to gain) |

