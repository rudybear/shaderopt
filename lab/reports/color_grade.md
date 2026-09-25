# color_grade

Every variant measured against the same device's baseline, interleaved. Speedup is the relative reduction of the touched pass's median GPU time with a bootstrap 95% CI. Error is the worst FLIP p99 over the judged outputs. Gates: 1 valid, 2 no new validation messages, 3 quality within the effective tolerance, 4 timing (>= max(2%, noise floor, min_speedup), CI excludes 0). `strict` gates use no tolerance. Variants live in `lab/variants/color_grade/`.

## Device 6554455c23490514

### Pareto frontier (error vs speedup)

| variant | worst FLIP p99 | min speedup (train) | scenarios (train/holdout) | accepted (budget) | accepted (strict) |
|---|---|---|---|---|---|
| hyp_foldmat | 0.0000 | +1.24% | 2/1 | no | no |

### Accepted variants

- none

### All variants

| variant | mode | set | sites | worst FLIP p99 | min speedup | accepted | strict |
|---|---|---|---|---|---|---|---|
| hyp_foldmat |  |  |  | 0.0000 | +1.24% | no | no |
| g_foldmat_xh_a0p01_dzero |  |  |  | 0.0000 | +1.15% | no | no |
| g_foldmat_-h_a0p001_dnone |  |  |  | 0.0000 | +1.09% | no | no |
| g_foldmat_xh_a0p01_dnone |  |  |  | 0.0000 | +1.09% | no | no |
| g_foldmat_xh_a0p001_dnone |  |  |  | 0.0000 | +1.06% | no | no |
| g_foldmat_xh_aoff_dzero |  |  |  | 0.0000 | +1.06% | no | no |
| g_baseline_xh_a0p01_dzero |  |  |  | 0.0000 | +1.00% | no | no |
| g_baseline_xh_a0p001_dzero |  |  |  | 0.0000 | +0.99% | no | no |
| g_foldmat_xh_a0p001_dzero |  |  |  | 0.0000 | +0.97% | no | no |
| hoist |  |  |  | 0.0000 | +0.92% | no | no |
| g_foldmat_-h_a0p001_dq4 |  |  |  | 0.0158 | -0.06% | no | no |
| canon_full |  |  |  | 0.0000 | -0.09% | no | no |
| canon_exact |  |  |  | 0.0000 | -0.09% | no | no |
| g_foldmat_xh_a0p01_dq4 |  |  |  | 0.0158 | -0.17% | no | no |
| g_foldmat_-h_a0p0001_dq4 |  |  |  | 0.0158 | -0.20% | no | no |
| g_foldmat_xh_a0p001_dq4 |  |  |  | 0.0158 | -0.20% | no | no |
| demote_f16_s33 | f16 | s33 | 1 | 0.0005 | -0.27% | no | no |
| demote_f16_s37 | f16 | s37 | 1 | 0.0013 | -0.27% | no | no |
| g_foldmat_-h_a0p01_dq4 |  |  |  | 0.0158 | -0.32% | no | no |
| g_foldmat_xh_a0p0001_dq4 |  |  |  | 0.0158 | -0.38% | no | no |
| demote_f16_q2 | f16 | q2 | 45 | 0.0308 | -2.68% | no | no |
| demote_relaxed_q4 | relaxed | q4 | 20 | 0.0145 | -2.84% | no | no |
| demote_relaxed_q2 | relaxed | q2 | 45 | 0.0308 | -3.46% | no | no |

