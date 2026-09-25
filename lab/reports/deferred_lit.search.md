# deferred_lit: M5 search

Genome = (source, exact, hoist, approx level, demote level, mode). 144 genomes enumerated and predicted in the CPU model against the ORIGINAL shader; 12 measured on the device (budget 8 + neighbours). Tolerance: {'metric': 'flip', 'mean_max': 0.01, 'p99_max': 0.05, 'min_speedup': 0.1}.

## Measured (Pareto on measured speedup vs measured error)

| genome | predicted p99 | measured worst p99 | min speedup (train) | accepted |
|---|---|---|---|---|
| g_pcf4_xh_a0p001_dnone | 0.0269 | 0.0268 | +14.13% | yes |
| g_pcf4_xh_a0p0001_dq1 | 0.0431 | 0.0431 | +14.11% | yes |
| g_pcf4_--_a0p001_dall | 0.0431 | 0.0431 | +14.11% | yes |
| g_pcf4_xh_a0p001_dq2 | 0.0431 | 0.0431 | +14.08% | yes |
| g_pcf4_xh_a0p01_dq1 | 0.0431 | 0.0431 | +14.07% | yes |
| g_pcf4_xh_a0p001_dall | 0.0431 | 0.0431 | +14.04% | yes |
| g_pcf4_xh_a0p01_dall | 0.0431 | 0.0431 | +14.03% | yes |
| g_pcf4_xh_a0p001_dq4 | 0.0431 | 0.0431 | +14.02% | yes |
| g_pcf4_xh_a0p001_dq1 | 0.0431 | 0.0431 | +13.98% | yes |
| g_pcf4_xh_a0p01_dq2 | 0.0431 | 0.0431 | +13.97% | yes |
| g_pcf4_xh_a0p01_dq4 | 0.0431 | 0.0431 | +13.97% | yes |
| g_baseline_xh_a0p01_dq1 | 0.0061 | 0.0055 | -0.09% | no |

## Predicted but not measured (within budget)

124 genomes; 0 predicted over budget; 0 failed to build.
