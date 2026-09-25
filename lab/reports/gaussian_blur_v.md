# gaussian_blur_v

Every variant measured against the same device's baseline, interleaved. Speedup is the relative reduction of the touched pass's median GPU time with a bootstrap 95% CI. Error is the worst FLIP p99 over the judged outputs. Gates: 1 valid, 2 no new validation messages, 3 quality within the effective tolerance, 4 timing (>= max(2%, noise floor, min_speedup), CI excludes 0). `strict` gates use no tolerance. Variants live in `lab/variants/gaussian_blur_v/`.

## Device 6554455c23490514

### Pareto frontier (error vs speedup)

| variant | worst FLIP p99 | min speedup (train) | scenarios (train/holdout) | accepted (budget) | accepted (strict) |
|---|---|---|---|---|---|
| hoist | 0.0000 | +0.08% | 2/2 | no | no |
| hyp_bilinear5 | 0.0100 | +11.42% | 2/2 | yes | no |

### Accepted variants

- **hyp_bilinear5**: speedup +11.42%, worst FLIP p99 0.0100, tolerance budgets, 1 edit ops (`lab/variants/gaussian_blur_v/hyp_bilinear5/variant.json`)

### All variants

| variant | mode | set | sites | worst FLIP p99 | min speedup | accepted | strict |
|---|---|---|---|---|---|---|---|
| hyp_bilinear5 |  |  |  | 0.0100 | +11.42% | yes | no |
| demote_f16_q4 | f16 | q4 | 19 | 0.9674 | +4.08% | no | no |
| demote_f16_zero | f16 | zero | 8 | 0.9674 | +3.63% | no | no |
| hoist |  |  |  | 0.0000 | +0.08% | no | no |
| canon_exact |  |  |  | 0.0000 | +0.00% | no | no |
| demote_f16_s47 | f16 | s47 | 1 | 0.0000 | +0.00% | no | no |
| canon_full |  |  |  | 0.0000 | -0.04% | no | no |
| demote_f16_s46 | f16 | s46 | 1 | 0.0000 | -0.08% | no | no |
| approx_0p001 |  |  |  | 0.0051 | -0.28% | no | no |
| demote_relaxed_zero | relaxed | zero | 8 | 0.9674 | -0.32% | no | no |
| demote_relaxed_q4 | relaxed | q4 | 19 | 0.9674 | -0.81% | no | no |

