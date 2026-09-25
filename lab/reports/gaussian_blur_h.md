# gaussian_blur_h

Every variant measured against the same device's baseline, interleaved. Speedup is the relative reduction of the touched pass's median GPU time with a bootstrap 95% CI. Error is the worst FLIP p99 over the judged outputs. Gates: 1 valid, 2 no new validation messages, 3 quality within the effective tolerance, 4 timing (>= max(2%, noise floor, min_speedup), CI excludes 0). `strict` gates use no tolerance. Variants live in `lab/variants/gaussian_blur_h/`.

## Device 6554455c23490514

### Pareto frontier (error vs speedup)

| variant | worst FLIP p99 | min speedup (train) | scenarios (train/holdout) | accepted (budget) | accepted (strict) |
|---|---|---|---|---|---|
| canon_full | 0.0000 | +0.00% | 2/0 | no | no |
| hyp_bilinear5 | 0.0071 | +11.28% | 2/2 | yes | no |

### Accepted variants

- **hyp_bilinear5**: speedup +11.28%, worst FLIP p99 0.0071, tolerance budgets, 1 edit ops (`lab/variants/gaussian_blur_h/hyp_bilinear5/variant.json`)

### All variants

| variant | mode | set | sites | worst FLIP p99 | min speedup | accepted | strict |
|---|---|---|---|---|---|---|---|
| hyp_bilinear5 |  |  |  | 0.0071 | +11.28% | yes | no |
| canon_full |  |  |  | 0.0000 | +0.00% | no | no |
| canon_exact |  |  |  | 0.0000 | -0.12% | no | no |

