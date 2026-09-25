# bloom_composite: M5 search

Genome = (source, exact, hoist, approx level, demote level, mode). 72 genomes enumerated and predicted in the CPU model against the ORIGINAL shader; 6 measured on the device (budget 8 + neighbours). Tolerance: {'metric': 'flip', 'mean_max': 0.01, 'p99_max': 0.05, 'min_speedup': 0.1}.

## Measured (Pareto on measured speedup vs measured error)

| genome | predicted p99 | measured worst p99 | min speedup (train) | accepted |
|---|---|---|---|---|
| g_baseline_xh_a0p0001_dzero | 0.0000 | 0.0000 | +0.03% | no |
| g_baseline_xh_aoff_dnone | 0.0000 | 0.0000 | +0.03% | no |
| g_baseline_xh_a0p001_dnone | 0.0000 | 0.0000 | -0.08% | no |
| g_baseline_xh_aoff_dzero | 0.0000 | 0.0000 | -0.08% | no |
| g_baseline_xh_a0p0001_dnone | 0.0000 | 0.0000 | -0.11% | no |
| g_baseline_xh_a0p001_dzero | 0.0000 | 0.0000 | -0.13% | no |

## Predicted but not measured (within budget)

0 genomes; 54 predicted over budget; 0 failed to build.
