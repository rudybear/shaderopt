# vignette_grain

Every variant measured against the same device's baseline, interleaved. Speedup is the relative reduction of the touched pass's median GPU time with a bootstrap 95% CI. Error is the worst FLIP p99 over the judged outputs. Gates: 1 valid, 2 no new validation messages, 3 quality within the effective tolerance, 4 timing (>= max(2%, noise floor, min_speedup), CI excludes 0). `strict` gates use no tolerance. Variants live in `lab/variants/vignette_grain/`.

## Device 6554455c23490514

### Pareto frontier (error vs speedup)

| variant | worst FLIP p99 | min speedup (train) | scenarios (train/holdout) | accepted (budget) | accepted (strict) |
|---|---|---|---|---|---|
| g_baseline_xh_a0p001_dzero | 0.0000 | +0.15% | 1/1 | no | no |
| hyp_cheaphash | 0.4636 | +0.15% | 1/1 | no | no |

### Accepted variants

- none

### All variants

| variant | mode | set | sites | worst FLIP p99 | min speedup | accepted | strict |
|---|---|---|---|---|---|---|---|
| hyp_cheaphash |  |  |  | 0.4636 | +0.15% | no | no |
| g_baseline_xh_a0p001_dzero |  |  |  | 0.0000 | +0.15% | no | no |
| demote_f16_all | f16 | all | 37 | 0.1489 | +0.06% | no | no |
| g_baseline_-h_a0p001_dzero |  |  |  | 0.0000 | +0.03% | no | no |
| g_baseline_xh_a0p0001_dnone |  |  |  | 0.0000 | +0.03% | no | no |
| g_baseline_xh_a0p01_dnone |  |  |  | 0.0000 | +0.03% | no | no |
| canon_exact |  |  |  | 0.0000 | +0.00% | no | no |
| g_baseline_xh_aoff_dzero |  |  |  | 0.0000 | +0.00% | no | no |
| g_baseline_xh_aoff_dnone |  |  |  | 0.0000 | -0.03% | no | no |
| g_baseline_xh_a0p01_dzero |  |  |  | 0.0000 | -0.03% | no | no |
| demote_relaxed_zero | relaxed | zero | 1 | 0.0000 | -0.03% | no | no |
| demote_f16_s98 | f16 | s98 | 1 | 0.0000 | -0.03% | no | no |
| demote_f16_zero | f16 | zero | 1 | 0.0000 | -0.03% | no | no |
| g_baseline_-h_a0p0001_dzero |  |  |  | 0.0000 | -0.06% | no | no |
| g_baseline_xh_a0p0001_dzero |  |  |  | 0.0000 | -0.06% | no | no |
| g_baseline_-h_a0p01_dzero |  |  |  | 0.0000 | -0.06% | no | no |
| canon_full |  |  |  | 0.0000 | -0.09% | no | no |
| g_baseline_--_aoff_dzero |  |  |  | 0.0000 | -0.12% | no | no |
| demote_f16_q4 | f16 | q4 | 24 | 0.1491 | -0.21% | no | no |
| demote_f16_s103 | f16 | s103 | 1 | 0.0020 | -0.27% | no | no |
| demote_f16_q2 | f16 | q2 | 25 | 0.1491 | -0.30% | no | no |
| demote_relaxed_q4 | relaxed | q4 | 24 | 0.0233 | -1.19% | no | no |
| demote_relaxed_q2 | relaxed | q2 | 25 | 0.0233 | -1.28% | no | no |
| demote_relaxed_all | relaxed | all | 37 | 0.2951 | -2.20% | no | no |

