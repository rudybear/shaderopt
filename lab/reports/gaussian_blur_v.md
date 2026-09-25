# gaussian_blur_v

Every variant measured against the same device's baseline, interleaved. Speedup is the relative reduction of the touched pass's median GPU time with a bootstrap 95% CI. Error is the worst FLIP p99 over the judged outputs. Gates: 1 valid, 2 no new validation messages, 3 quality within the effective tolerance, 4 timing (>= max(2%, noise floor, min_speedup), CI excludes 0). `strict` gates use no tolerance. Variants live in `lab/variants/gaussian_blur_v/`.

## Device 6554455c23490514

### Pareto frontier (error vs speedup)

| variant | worst FLIP p99 | min speedup (train) | scenarios (train/holdout) | accepted (budget) | accepted (strict) |
|---|---|---|---|---|---|
| g_baseline_xh_a0p0001_dnone | 0.0000 | +0.20% | 2/2 | no | no |
| g_bilinear5_-h_a0p001_dnone | 0.0100 | +11.93% | 2/2 | yes | no |

### Accepted variants

- **g_bilinear5_-h_a0p001_dnone**: speedup +11.93%, worst FLIP p99 0.0100, tolerance budgets, 1 edit ops (`lab/variants/gaussian_blur_v/g_bilinear5_-h_a0p001_dnone/variant.json`)
- **g_bilinear5_-h_a0p01_dnone**: speedup +11.86%, worst FLIP p99 0.0100, tolerance budgets, 1 edit ops (`lab/variants/gaussian_blur_v/g_bilinear5_-h_a0p01_dnone/variant.json`)
- **g_bilinear5_-h_a0p0001_dnone**: speedup +11.85%, worst FLIP p99 0.0100, tolerance budgets, 1 edit ops (`lab/variants/gaussian_blur_v/g_bilinear5_-h_a0p0001_dnone/variant.json`)
- **g_bilinear5_xh_a0p001_dnone**: speedup +11.81%, worst FLIP p99 0.0100, tolerance budgets, 1 edit ops (`lab/variants/gaussian_blur_v/g_bilinear5_xh_a0p001_dnone/variant.json`)
- **g_bilinear5_xh_aoff_dnone**: speedup +11.76%, worst FLIP p99 0.0100, tolerance budgets, 1 edit ops (`lab/variants/gaussian_blur_v/g_bilinear5_xh_aoff_dnone/variant.json`)
- **g_bilinear5_xh_a0p01_dnone**: speedup +11.75%, worst FLIP p99 0.0100, tolerance budgets, 1 edit ops (`lab/variants/gaussian_blur_v/g_bilinear5_xh_a0p01_dnone/variant.json`)
- **g_bilinear5_-h_aoff_dnone**: speedup +11.73%, worst FLIP p99 0.0100, tolerance budgets, 1 edit ops (`lab/variants/gaussian_blur_v/g_bilinear5_-h_aoff_dnone/variant.json`)
- **g_bilinear5_xh_a0p0001_dnone**: speedup +11.64%, worst FLIP p99 0.0100, tolerance budgets, 1 edit ops (`lab/variants/gaussian_blur_v/g_bilinear5_xh_a0p0001_dnone/variant.json`)
- **g_bilinear5_--_a0p0001_dnone**: speedup +11.43%, worst FLIP p99 0.0100, tolerance budgets, 1 edit ops (`lab/variants/gaussian_blur_v/g_bilinear5_--_a0p0001_dnone/variant.json`)
- **hyp_bilinear5**: speedup +11.42%, worst FLIP p99 0.0100, tolerance budgets, 1 edit ops (`lab/variants/gaussian_blur_v/hyp_bilinear5/variant.json`)
- **g_bilinear5_--_aoff_dnone**: speedup +11.28%, worst FLIP p99 0.0100, tolerance budgets, 1 edit ops (`lab/variants/gaussian_blur_v/g_bilinear5_--_aoff_dnone/variant.json`)

### All variants

| variant | mode | set | sites | worst FLIP p99 | min speedup | accepted | strict |
|---|---|---|---|---|---|---|---|
| g_bilinear5_-h_a0p001_dnone |  |  |  | 0.0100 | +11.93% | yes | no |
| g_bilinear5_-h_a0p01_dnone |  |  |  | 0.0100 | +11.86% | yes | no |
| g_bilinear5_-h_a0p0001_dnone |  |  |  | 0.0100 | +11.85% | yes | no |
| g_bilinear5_xh_a0p01_dall |  |  |  | 0.9674 | +11.85% | no | no |
| g_bilinear5_xh_a0p001_dall |  |  |  | 0.9674 | +11.85% | no | no |
| g_bilinear5_xh_a0p0001_dq4 |  |  |  | 0.9674 | +11.85% | no | no |
| g_bilinear5_xh_a0p001_dnone |  |  |  | 0.0100 | +11.81% | yes | no |
| g_bilinear5_xh_a0p0001_dq1 |  |  |  | 0.9674 | +11.80% | no | no |
| g_bilinear5_xh_a0p01_dq2 |  |  |  | 0.9674 | +11.78% | no | no |
| g_bilinear5_xh_a0p001_dq4 |  |  |  | 0.9674 | +11.77% | no | no |
| g_bilinear5_xh_a0p001_dq2 |  |  |  | 0.9674 | +11.77% | no | no |
| g_bilinear5_xh_a0p001_dq1 |  |  |  | 0.9674 | +11.77% | no | no |
| g_bilinear5_xh_aoff_dnone |  |  |  | 0.0100 | +11.76% | yes | no |
| g_bilinear5_xh_a0p01_dnone |  |  |  | 0.0100 | +11.75% | yes | no |
| g_bilinear5_-h_aoff_dnone |  |  |  | 0.0100 | +11.73% | yes | no |
| g_bilinear5_xh_a0p01_dq4 |  |  |  | 0.9674 | +11.70% | no | no |
| g_bilinear5_xh_a0p0001_dnone |  |  |  | 0.0100 | +11.64% | yes | no |
| g_bilinear5_xh_a0p01_dq1 |  |  |  | 0.9674 | +11.60% | no | no |
| g_bilinear5_--_a0p0001_dnone |  |  |  | 0.0100 | +11.43% | yes | no |
| hyp_bilinear5 |  |  |  | 0.0100 | +11.42% | yes | no |
| g_bilinear5_--_aoff_dnone |  |  |  | 0.0100 | +11.28% | yes | no |
| g_bilinear5_--_a0p01_dq2 |  |  |  | 0.9674 | +11.15% | no | no |
| g_bilinear5_--_a0p01_dall |  |  |  | 0.9674 | +11.13% | no | no |
| g_baseline_xh_a0p01_dq1 |  |  |  | 0.9674 | +4.14% | no | no |
| g_baseline_xh_a0p01_dq4 |  |  |  | 0.9674 | +4.11% | no | no |
| demote_f16_q4 | f16 | q4 | 19 | 0.9674 | +4.08% | no | no |
| demote_f16_zero | f16 | zero | 8 | 0.9674 | +3.63% | no | no |
| g_baseline_xh_a0p0001_dnone |  |  |  | 0.0000 | +0.20% | no | no |
| hoist |  |  |  | 0.0000 | +0.08% | no | no |
| canon_exact |  |  |  | 0.0000 | +0.00% | no | no |
| demote_f16_s47 | f16 | s47 | 1 | 0.0000 | +0.00% | no | no |
| canon_full |  |  |  | 0.0000 | -0.04% | no | no |
| demote_f16_s46 | f16 | s46 | 1 | 0.0000 | -0.08% | no | no |
| approx_0p001 |  |  |  | 0.0051 | -0.28% | no | no |
| g_baseline_-h_a0p0001_dnone |  |  |  | 0.0019 | -0.32% | no | no |
| demote_relaxed_zero | relaxed | zero | 8 | 0.9674 | -0.32% | no | no |
| demote_relaxed_q4 | relaxed | q4 | 19 | 0.9674 | -0.81% | no | no |

