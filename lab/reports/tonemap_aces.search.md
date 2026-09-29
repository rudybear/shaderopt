# tonemap_aces: M5 search

Genome = (source, exact, hoist, approx level, demote level, mode). 144 genomes enumerated and predicted in the CPU model against the ORIGINAL shader; 9 measured on the device (budget 6 + neighbours). Tolerance: {'metric': 'flip', 'mean_max': 0.01, 'p99_max': 0.05, 'min_speedup': 0.1}.

## Measured (Pareto on measured speedup vs measured error)

| genome | predicted p99 | measured worst p99 | min speedup (train) | accepted |
|---|---|---|---|---|
| g_onepow_xh_a0p001_dq1 | 0.0270 | 0.0335 | +0.23% | no |
| g_onepow_xh_aoff_dq2 | 0.0270 | 0.0335 | -0.47% | no |
| g_onepow_xh_aoff_dq1 | 0.0270 | 0.0335 | -1.03% | no |
| g_onepow_xh_a0p01_dq1 | 0.0270 | 0.0335 | -9.46% | no |
| g_onepow_--_a0p01_dq2 | 0.0270 | 0.0335 | -23.54% | no |
| g_onepow_xh_a0p01_dq2 | 0.0270 | 0.0335 | -29.02% | no |
| g_onepow_xh_a0p001_dall | 0.0270 | 0.0335 | -35.88% | no |
| g_onepow_xh_a0p001_dq2 | 0.0270 | 0.0335 | -38.16% | no |
| g_onepow_xh_a0p01_dall | 0.0270 | 0.0335 | -70.70% | no |

## Predicted but not measured (within budget)

127 genomes; 0 predicted over budget; 0 failed to build.
