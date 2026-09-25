# fxaa

Every variant measured against the same device's baseline, interleaved. Speedup is the relative reduction of the touched pass's median GPU time with a bootstrap 95% CI. Error is the worst FLIP p99 over the judged outputs. Gates: 1 valid, 2 no new validation messages, 3 quality within the effective tolerance, 4 timing (>= max(2%, noise floor, min_speedup), CI excludes 0). `strict` gates use no tolerance. Variants live in `lab/variants/fxaa/`.

## Device 6554455c23490514

### Pareto frontier (error vs speedup)

| variant | worst FLIP p99 | min speedup (train) | scenarios (train/holdout) | accepted (budget) | accepted (strict) |
|---|---|---|---|---|---|
| demote_f16_s138 | 0.0000 | +0.03% | 2/2 | no | no |
| hyp_dir4 | 0.2884 | +2.45% | 2/2 | no | no |

### Accepted variants

- none

### All variants

| variant | mode | set | sites | worst FLIP p99 | min speedup | accepted | strict |
|---|---|---|---|---|---|---|---|
| hyp_dir4 |  |  |  | 0.2884 | +2.45% | no | no |
| demote_f16_s138 | f16 | s138 | 1 | 0.0000 | +0.03% | no | no |
| canon_full |  |  |  | 0.0000 | +0.01% | no | no |
| demote_relaxed_zero | relaxed | zero | 29 | 0.0151 | -0.02% | no | no |
| hoist |  |  |  | 0.0000 | -0.02% | no | no |
| canon_exact |  |  |  | 0.0000 | -0.08% | no | no |
| demote_f16_zero | f16 | zero | 29 | 0.0151 | -0.10% | no | no |
| demote_f16_s137 | f16 | s137 | 1 | 0.0000 | -0.10% | no | no |

