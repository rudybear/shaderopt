# bloom_composite

Every variant measured against the same device's baseline, interleaved. Speedup is the relative reduction of the touched pass's median GPU time with a bootstrap 95% CI. Error is the worst FLIP p99 over the judged outputs. Gates: 1 valid, 2 no new validation messages, 3 quality within the effective tolerance, 4 timing (>= max(2%, noise floor, min_speedup), CI excludes 0). `strict` gates use no tolerance. Variants live in `lab/variants/bloom_composite/`.

## Device 6554455c23490514

### Pareto frontier (error vs speedup)

| variant | worst FLIP p99 | min speedup (train) | scenarios (train/holdout) | accepted (budget) | accepted (strict) |
|---|---|---|---|---|---|
| canon_full | 0.0000 | +0.00% | 2/0 | no | no |

### Accepted variants

- none

### All variants

| variant | mode | set | sites | worst FLIP p99 | min speedup | accepted | strict |
|---|---|---|---|---|---|---|---|
| canon_full |  |  |  | 0.0000 | +0.00% | no | no |
| canon_exact |  |  |  | 0.0000 | -0.08% | no | no |
| demote_f16_s55 | f16 | s55 | 1 | 0.0108 | -0.11% | no | no |
| demote_relaxed_q4 | relaxed | q4 | 15 | 0.9674 | -0.21% | no | no |
| demote_f16_s33 | f16 | s33 | 1 | 0.9674 | -0.24% | no | no |
| demote_f16_q4 | f16 | q4 | 15 | 0.9674 | -0.40% | no | no |

