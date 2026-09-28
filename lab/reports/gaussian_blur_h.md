# gaussian_blur_h

Every variant measured against the same device's baseline, interleaved. Speedup is the relative reduction of the touched pass's median GPU time with a bootstrap 95% CI. Error is the worst FLIP p99 over the judged outputs. Gates: 1 valid, 2 no new validation messages, 3 quality within the effective tolerance, 4 timing (>= max(2%, noise floor, min_speedup), CI excludes 0). `strict` gates use no tolerance. Variants live in `lab/variants/gaussian_blur_h/`.

## Device 6554455c23490514

### Pareto frontier (error vs speedup)

| variant | worst FLIP p99 | min speedup (train) | scenarios (train/holdout) | accepted (budget) | accepted (strict) |
|---|---|---|---|---|---|
| hoist | 0.0000 | +0.16% | 2/2 | no | no |
| g_bilinear5_xh_a0p01_dnone | 0.0071 | +11.46% | 2/2 | yes | no |

### Accepted variants

- **g_bilinear5_xh_a0p01_dnone**: speedup +11.46%, worst FLIP p99 0.0071, tolerance budgets, 1 edit ops (`lab/variants/gaussian_blur_h/g_bilinear5_xh_a0p01_dnone/variant.json`)
- **g_bilinear5_xh_aoff_dnone**: speedup +11.43%, worst FLIP p99 0.0071, tolerance budgets, 1 edit ops (`lab/variants/gaussian_blur_h/g_bilinear5_xh_aoff_dnone/variant.json`)
- **g_bilinear5_-h_a0p01_dnone**: speedup +11.42%, worst FLIP p99 0.0071, tolerance budgets, 1 edit ops (`lab/variants/gaussian_blur_h/g_bilinear5_-h_a0p01_dnone/variant.json`)
- **g_bilinear5_-h_a0p0001_dnone**: speedup +11.40%, worst FLIP p99 0.0071, tolerance budgets, 1 edit ops (`lab/variants/gaussian_blur_h/g_bilinear5_-h_a0p0001_dnone/variant.json`)
- **g_bilinear5_xh_a0p0001_dnone**: speedup +11.37%, worst FLIP p99 0.0071, tolerance budgets, 1 edit ops (`lab/variants/gaussian_blur_h/g_bilinear5_xh_a0p0001_dnone/variant.json`)
- **g_bilinear5_-h_aoff_dnone**: speedup +11.37%, worst FLIP p99 0.0071, tolerance budgets, 1 edit ops (`lab/variants/gaussian_blur_h/g_bilinear5_-h_aoff_dnone/variant.json`)
- **g_bilinear5_--_a0p0001_dnone**: speedup +11.33%, worst FLIP p99 0.0071, tolerance budgets, 1 edit ops (`lab/variants/gaussian_blur_h/g_bilinear5_--_a0p0001_dnone/variant.json`)
- **g_bilinear5_--_a0p01_dnone**: speedup +11.32%, worst FLIP p99 0.0071, tolerance budgets, 1 edit ops (`lab/variants/gaussian_blur_h/g_bilinear5_--_a0p01_dnone/variant.json`)
- **g_bilinear5_-h_a0p001_dnone**: speedup +11.29%, worst FLIP p99 0.0071, tolerance budgets, 1 edit ops (`lab/variants/gaussian_blur_h/g_bilinear5_-h_a0p001_dnone/variant.json`)
- **hyp_bilinear5**: speedup +11.29%, worst FLIP p99 0.0072, tolerance budgets, 1 edit ops (`lab/variants/gaussian_blur_h/hyp_bilinear5/variant.json`)
- **g_bilinear5_--_a0p001_dnone**: speedup +11.22%, worst FLIP p99 0.0071, tolerance budgets, 1 edit ops (`lab/variants/gaussian_blur_h/g_bilinear5_--_a0p001_dnone/variant.json`)
- **g_bilinear5_xh_a0p001_dnone**: speedup +11.17%, worst FLIP p99 0.0071, tolerance budgets, 1 edit ops (`lab/variants/gaussian_blur_h/g_bilinear5_xh_a0p001_dnone/variant.json`)

### All variants

| variant | mode | set | sites | worst FLIP p99 | min speedup | accepted | strict |
|---|---|---|---|---|---|---|---|
| g_bilinear5_xh_a0p01_dnone |  |  |  | 0.0071 | +11.46% | yes | no |
| g_bilinear5_xh_aoff_dnone |  |  |  | 0.0071 | +11.43% | yes | no |
| g_bilinear5_-h_a0p01_dnone |  |  |  | 0.0071 | +11.42% | yes | no |
| g_bilinear5_-h_a0p0001_dnone |  |  |  | 0.0071 | +11.40% | yes | no |
| g_bilinear5_xh_a0p001_dq4 |  |  |  | 0.9674 | +11.40% | no | no |
| g_bilinear5_xh_a0p01_dq2 |  |  |  | 0.9674 | +11.38% | no | no |
| g_bilinear5_xh_a0p0001_dnone |  |  |  | 0.0071 | +11.37% | yes | no |
| g_bilinear5_-h_aoff_dnone |  |  |  | 0.0071 | +11.37% | yes | no |
| g_bilinear5_xh_a0p01_dall |  |  |  | 0.9674 | +11.36% | no | no |
| g_bilinear5_xh_a0p001_dq2 |  |  |  | 0.9674 | +11.36% | no | no |
| g_bilinear5_xh_a0p001_dq1 |  |  |  | 0.9674 | +11.34% | no | no |
| g_bilinear5_--_a0p0001_dnone |  |  |  | 0.0071 | +11.33% | yes | no |
| g_bilinear5_--_a0p01_dnone |  |  |  | 0.0071 | +11.32% | yes | no |
| g_bilinear5_xh_a0p001_dall |  |  |  | 0.9674 | +11.30% | no | no |
| g_bilinear5_-h_a0p001_dnone |  |  |  | 0.0071 | +11.29% | yes | no |
| hyp_bilinear5 |  |  |  | 0.0072 | +11.29% | yes | no |
| g_bilinear5_xh_a0p0001_dq1 |  |  |  | 0.9674 | +11.27% | no | no |
| g_bilinear5_xh_a0p0001_dq4 |  |  |  | 0.9674 | +11.26% | no | no |
| g_bilinear5_xh_a0p01_dq4 |  |  |  | 0.9674 | +11.25% | no | no |
| g_bilinear5_xh_a0p01_dq1 |  |  |  | 0.9674 | +11.24% | no | no |
| g_bilinear5_--_a0p001_dnone |  |  |  | 0.0071 | +11.22% | yes | no |
| g_bilinear5_xh_a0p001_dnone |  |  |  | 0.0071 | +11.17% | yes | no |
| g_bilinear5_--_a0p01_dq4 |  |  |  | 0.9674 | +11.14% | no | no |
| g_bilinear5_--_a0p01_dq1 |  |  |  | 0.9674 | +11.09% | no | no |
| g_bilinear5_--_a0p01_dall |  |  |  | 0.9674 | +11.08% | no | no |
| g_bilinear5_--_a0p001_dq1 |  |  |  | 0.9674 | +11.07% | no | no |
| g_bilinear5_--_a0p001_dall |  |  |  | 0.9674 | +11.06% | no | no |
| g_bilinear5_--_a0p001_dzero |  |  |  | 0.9674 | +11.05% | no | no |
| g_bilinear5_--_a0p001_dq4 |  |  |  | 0.9674 | +11.04% | no | no |
| g_bilinear5_--_a0p001_dq2 |  |  |  | 0.9674 | +11.03% | no | no |
| g_bilinear5_--_a0p01_dq2 |  |  |  | 0.9674 | +11.01% | no | no |
| demote_f16_q4 | f16 | q4 | 19 | 0.9674 | +3.56% | no | no |
| g_baseline_--_a0p01_dq2 |  |  |  | 0.9674 | +3.43% | no | no |
| g_baseline_xh_a0p001_dq4 |  |  |  | 0.9674 | +3.31% | no | no |
| g_baseline_xh_a0p001_dq1 |  |  |  | 0.9674 | +3.30% | no | no |
| demote_f16_zero | f16 | zero | 8 | 0.9674 | +3.19% | no | no |
| hoist |  |  |  | 0.0000 | +0.16% | no | no |
| canon_full |  |  |  | 0.0000 | +0.00% | no | no |
| canon_exact |  |  |  | 0.0000 | -0.12% | no | no |
| demote_f16_s46 | f16 | s46 | 1 | 0.0000 | -0.12% | no | no |
| demote_f16_s47 | f16 | s47 | 1 | 0.0000 | -0.16% | no | no |
| g_baseline_-h_a0p0001_dnone |  |  |  | 0.0052 | -0.24% | no | no |
| approx_0p001 |  |  |  | 0.0052 | -0.28% | no | no |
| demote_relaxed_zero | relaxed | zero | 8 | 0.9674 | -0.32% | no | no |
| demote_relaxed_q4 | relaxed | q4 | 19 | 0.9674 | -0.65% | no | no |

## Device c59fe355f04b781f

### Pareto frontier (error vs speedup)

| variant | worst FLIP p99 | min speedup (train) | scenarios (train/holdout) | accepted (budget) | accepted (strict) |
|---|---|---|---|---|---|
| hyp_bilinear5 | 0.0071 | +19.33% | 2/2 | no | no |

### Accepted variants

- none

### All variants

| variant | mode | set | sites | worst FLIP p99 | min speedup | accepted | strict |
|---|---|---|---|---|---|---|---|
| hyp_bilinear5 |  |  |  | 0.0071 | +19.33% | no | no |

