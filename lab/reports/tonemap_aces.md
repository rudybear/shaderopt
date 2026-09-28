# tonemap_aces

Every variant measured against the same device's baseline, interleaved. Speedup is the relative reduction of the touched pass's median GPU time with a bootstrap 95% CI. Error is the worst FLIP p99 over the judged outputs. Gates: 1 valid, 2 no new validation messages, 3 quality within the effective tolerance, 4 timing (>= max(2%, noise floor, min_speedup), CI excludes 0). `strict` gates use no tolerance. Variants live in `lab/variants/tonemap_aces/`.

## Device 6554455c23490514

### Pareto frontier (error vs speedup)

| variant | worst FLIP p99 | min speedup (train) | scenarios (train/holdout) | accepted (budget) | accepted (strict) |
|---|---|---|---|---|---|
| g_onepow_-h_a0p01_dnone | 0.0000 | +0.16% | 2/3 | no | no |
| demote_f16_q2 | 0.9674 | +1.09% | 2/3 | no | no |

### Accepted variants

- none

### All variants

| variant | mode | set | sites | worst FLIP p99 | min speedup | accepted | strict |
|---|---|---|---|---|---|---|---|
| demote_f16_q2 | f16 | q2 | 32 | 0.9674 | +1.09% | no | no |
| g_onepow_-h_a0p01_dnone |  |  |  | 0.0000 | +0.16% | no | no |
| g_onepow_-h_a0p001_dnone |  |  |  | 0.0000 | +0.13% | no | no |
| g_onepow_xh_a0p01_dnone |  |  |  | 0.0000 | +0.10% | no | no |
| g_onepow_-h_a0p0001_dnone |  |  |  | 0.0000 | +0.10% | no | no |
| g_onepow_--_a0p01_dnone |  |  |  | 0.0000 | +0.10% | no | no |
| g_onepow_xh_aoff_dnone |  |  |  | 0.0000 | +0.10% | no | no |
| g_onepow_--_a0p001_dnone |  |  |  | 0.0000 | +0.10% | no | no |
| g_onepow_-h_aoff_dnone |  |  |  | 0.0000 | +0.08% | no | no |
| g_baseline_xh_a0p001_dnone |  |  |  | 0.0000 | +0.08% | no | no |
| g_onepow_xh_a0p001_dnone |  |  |  | 0.0000 | +0.05% | no | no |
| g_onepow_xh_a0p0001_dnone |  |  |  | 0.0000 | +0.05% | no | no |
| canon_exact |  |  |  | 0.0000 | +0.03% | no | no |
| canon_full |  |  |  | 0.0000 | +0.00% | no | no |
| hyp_onepow |  |  |  | 0.0000 | +0.00% | no | no |
| g_baseline_xh_a0p01_dnone |  |  |  | 0.0000 | +0.00% | no | no |
| g_onepow_--_aoff_dnone |  |  |  | 0.0000 | +0.00% | no | no |
| g_baseline_xh_aoff_dnone |  |  |  | 0.0000 | -0.08% | no | no |
| demote_f16_s66 | f16 | s66 | 1 | 0.0000 | -0.10% | no | no |
| demote_f16_zero | f16 | zero | 28 | 0.9674 | -0.21% | no | no |
| demote_relaxed_zero | relaxed | zero | 28 | 0.9674 | -0.42% | no | no |
| demote_relaxed_q2 | relaxed | q2 | 32 | 0.9674 | -0.88% | no | no |
| demote_f16_s61 | f16 | s61 | 1 | 0.9674 | -0.92% | no | no |
| hyp_lut1d |  |  |  | 0.9674 | -61.05% | no | no |

## Device c59fe355f04b781f

### Pareto frontier (error vs speedup)

| variant | worst FLIP p99 | min speedup (train) | scenarios (train/holdout) | accepted (budget) | accepted (strict) |
|---|---|---|---|---|---|
| hyp_onepow | 0.0000 | -13.73% | 2/3 | no | no |
| demote_f16_s61 | 0.0160 | +3.30% | 2/3 | no | no |
| demote_f16_zero | 0.0300 | +21.55% | 2/3 | no | no |
| demote_relaxed_q2 | 0.0335 | +36.62% | 2/3 | no | no |

### Accepted variants

- none

### All variants

| variant | mode | set | sites | worst FLIP p99 | min speedup | accepted | strict |
|---|---|---|---|---|---|---|---|
| demote_relaxed_q2 | relaxed | q2 | 32 | 0.0335 | +36.62% | no | no |
| demote_f16_q2 | f16 | q2 | 32 | 0.0335 | +33.01% | no | no |
| demote_f16_zero | f16 | zero | 28 | 0.0300 | +21.55% | no | no |
| hyp_lut1d |  |  |  | 0.9674 | +9.14% | no | no |
| demote_f16_s61 | f16 | s61 | 1 | 0.0160 | +3.30% | no | no |
| hyp_onepow |  |  |  | 0.0000 | -13.73% | no | no |
| demote_relaxed_zero | relaxed | zero | 28 | 0.0300 | -24.68% | no | no |

