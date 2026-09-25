# vignette_grain

Every variant measured against the same device's baseline, interleaved. Speedup is the relative reduction of the touched pass's median GPU time with a bootstrap 95% CI. Error is the worst FLIP p99 over the judged outputs. Gates: 1 valid, 2 no new validation messages, 3 quality within the effective tolerance, 4 timing (>= max(2%, noise floor, min_speedup), CI excludes 0). `strict` gates use no tolerance. Variants live in `lab/variants/vignette_grain/`.

## Device 6554455c23490514

### Pareto frontier (error vs speedup)

| variant | worst FLIP p99 | min speedup (train) | scenarios (train/holdout) | accepted (budget) | accepted (strict) |
|---|---|---|---|---|---|
| canon_exact | 0.0000 | +0.00% | 1/0 | no | no |
| hyp_cheaphash | 0.4636 | +0.15% | 1/1 | no | no |

### Accepted variants

- none

### All variants

| variant | mode | set | sites | worst FLIP p99 | min speedup | accepted | strict |
|---|---|---|---|---|---|---|---|
| hyp_cheaphash |  |  |  | 0.4636 | +0.15% | no | no |
| canon_exact |  |  |  | 0.0000 | +0.00% | no | no |
| canon_full |  |  |  | 0.0000 | -0.09% | no | no |

