# deferred_lit: M5 search

Genome = (source, exact, hoist, approx level, demote level, mode). 144 genomes enumerated and predicted in the CPU model against the ORIGINAL shader; 12 measured on the device (budget 8 + neighbours). Tolerance: {'metric': 'flip', 'mean_max': 0.01, 'p99_max': 0.05, 'min_speedup': 0.1}.

## Measured (Pareto on measured speedup vs measured error)

| genome | predicted p99 | measured worst p99 | min speedup (train) | accepted |
|---|---|---|---|---|
| g_baseline_xh_a0p01_dzero | 0.0000 | 0.0000 | +0.04% | no |
| g_baseline_xh_a0p001_dnone | 0.0000 | 0.0000 | +0.04% | no |
| g_baseline_xh_a0p01_dnone | 0.0000 | 0.0000 | -0.01% | no |
| g_baseline_xh_a0p01_dall | 0.0079 | 0.0055 | -0.09% | no |
| g_baseline_xh_a0p001_dall | 0.0079 | 0.0055 | -0.12% | no |
| g_baseline_xh_a0p01_dq1 | 0.0079 | 0.0055 | -0.13% | no |
| g_baseline_xh_a0p001_dq2 | 0.0079 | 0.0055 | -0.15% | no |
| g_baseline_xh_a0p001_dq1 | 0.0079 | 0.0055 | -0.15% | no |
| g_baseline_xh_a0p001_dq4 | 0.0079 | 0.0055 | -0.16% | no |
| g_baseline_xh_a0p01_dq2 | 0.0079 | 0.0055 | -0.16% | no |
| g_baseline_xh_a0p01_dq4 | 0.0079 | 0.0055 | -0.31% | no |
| g_baseline_--_a0p001_dall | 0.0079 | 0.0055 | -0.34% | no |

## Predicted but not measured (within budget)

52 genomes; 72 predicted over budget; 0 failed to build.
