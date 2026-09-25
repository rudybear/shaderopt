# deferred_lit

Every variant measured against the same device's baseline, interleaved. Speedup is the relative reduction of the touched pass's median GPU time with a bootstrap 95% CI. Error is the worst FLIP p99 over the judged outputs. Gates: 1 valid, 2 no new validation messages, 3 quality within the effective tolerance, 4 timing (>= max(2%, noise floor, min_speedup), CI excludes 0). `strict` gates use no tolerance. Variants live in `lab/variants/deferred_lit/`.

## Device 6554455c23490514

### Pareto frontier (error vs speedup)

| variant | worst FLIP p99 | min speedup (train) | scenarios (train/holdout) | accepted (budget) | accepted (strict) |
|---|---|---|---|---|---|
| demote_f16_s130 | 0.0000 | +0.05% | 1/0 | no | no |
| hyp_pcf4 | 0.0268 | +11.70% | 1/0 | yes | no |

### Accepted variants

- **hyp_pcf4**: speedup +11.70%, worst FLIP p99 0.0268, tolerance budgets, 1 edit ops (`lab/variants/deferred_lit/hyp_pcf4/variant.json`)

### All variants

| variant | mode | set | sites | worst FLIP p99 | min speedup | accepted | strict |
|---|---|---|---|---|---|---|---|
| hyp_pcf4 |  |  |  | 0.0268 | +11.70% | yes | no |
| demote_f16_s130 | f16 | s130 | 1 | 0.0000 | +0.05% | no | no |
| demote_f16_s141 | f16 | s141 | 1 | 0.0000 | +0.05% | no | no |
| demote_f16_q4 | f16 | q4 | 38 | 0.0055 | -0.07% | no | no |
| demote_f16_zero | f16 | zero | 6 | 0.0000 | -0.07% | no | no |
| canon_exact |  |  |  | 0.0000 | -0.08% | no | no |
| canon_full |  |  |  | 0.0000 | -0.08% | no | no |
| demote_relaxed_zero | relaxed | zero | 6 | 0.0000 | -0.10% | no | no |
| demote_relaxed_q4 | relaxed | q4 | 38 | 0.0055 | -0.17% | no | no |

