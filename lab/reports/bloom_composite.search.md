# bloom_composite: M5 search

Genome = (source, exact, hoist, approx level, demote level, mode). 72 genomes enumerated and predicted in the CPU model against the ORIGINAL shader; 11 measured on the device (budget 8 + neighbours). Tolerance: {'metric': 'flip', 'mean_max': 0.01, 'p99_max': 0.05, 'min_speedup': 0.1}.

## Measured (Pareto on measured speedup vs measured error)

| genome | predicted p99 | measured worst p99 | min speedup (train) | accepted |
|---|---|---|---|---|
| g_baseline_xh_a0p001_dnone | 0.0000 | 0.0000 | -0.03% | no |
| g_baseline_xh_a0p0001_dall | 0.0222 | 0.9674 | -0.08% | no |
| g_baseline_xh_a0p001_dzero | 0.0000 | 0.0000 | -0.08% | no |
| g_baseline_xh_a0p001_dq4 | 0.0222 | 0.9674 | -0.11% | no |
| g_baseline_xh_a0p001_dq1 | 0.0222 | 0.9674 | -0.13% | no |
| g_baseline_xh_a0p001_dall | 0.0222 | 0.9674 | -0.13% | no |
| g_baseline_xh_a0p0001_dzero | 0.0000 | 0.0000 | -0.15% | no |
| g_baseline_xh_a0p0001_dq4 | 0.0222 | 0.9674 | -0.21% | no |
| g_baseline_xh_a0p0001_dq1 | 0.0222 | 0.9674 | -0.24% | no |
| g_baseline_xh_a0p0001_dq2 | 0.0222 | 0.9674 | -0.29% | no |
| g_baseline_xh_a0p001_dq2 | 0.0222 | 0.9674 | -0.43% | no |

## Predicted but not measured (within budget)

31 genomes; 18 predicted over budget; 0 failed to build.
