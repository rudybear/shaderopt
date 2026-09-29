# deferred_lit: M5 search

Genome = (source, exact, hoist, approx level, demote level, mode). 144 genomes enumerated and predicted in the CPU model against the ORIGINAL shader; 9 measured on the device (budget 6 + neighbours). Tolerance: {'metric': 'flip', 'mean_max': 0.01, 'p99_max': 0.05, 'min_speedup': 0.1}.

## Measured (Pareto on measured speedup vs measured error)

| genome | predicted p99 | measured worst p99 | min speedup (train) | accepted |
|---|---|---|---|---|
| g_pcf4_xh_a0p001_dall | 0.0384 | 0.0385 | +23.25% | yes |
| g_pcf4_xh_a0p001_dq2 | 0.0384 | 0.0385 | +22.53% | yes |
| g_pcf4_xh_a0p001_dq4 | 0.0384 | 0.0385 | +22.17% | yes |
| g_pcf4_xh_a0p01_dall | 0.0384 | 0.0385 | +21.76% | yes |
| g_pcf4_--_a0p01_dall | 0.0384 | 0.0385 | +21.61% | yes |
| g_pcf4_xh_a0p0001_dall | 0.0384 | 0.0385 | +21.38% | yes |
| g_pcf4_xh_a0p01_dq1 | 0.0384 | 0.0385 | +20.69% | yes |
| g_pcf4_xh_a0p01_dq4 | 0.0384 | 0.0385 | -3.30% | no |
| g_pcf4_xh_a0p01_dq2 | 0.0384 | 0.0385 | -16.64% | no |

## Predicted but not measured (within budget)

127 genomes; 0 predicted over budget; 0 failed to build.
