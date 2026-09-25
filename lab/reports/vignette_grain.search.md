# vignette_grain: M5 search

Genome = (source, exact, hoist, approx level, demote level, mode). 144 genomes enumerated and predicted in the CPU model against the ORIGINAL shader; 12 measured on the device (budget 8 + neighbours). Tolerance: {'metric': 'flip', 'mean_max': 0.01, 'p99_max': 0.05, 'min_speedup': 0.1}.

## Measured (Pareto on measured speedup vs measured error)

| genome | predicted p99 | measured worst p99 | min speedup (train) | accepted |
|---|---|---|---|---|
| g_baseline_xh_a0p001_dnone | 0.0000 | 0.0000 | +0.06% | no |
| g_baseline_xh_a0p001_dzero | 0.0000 | 0.0000 | +0.03% | no |
| g_baseline_xh_a0p0001_dzero | 0.0000 | 0.0000 | +0.00% | no |
| g_baseline_--_a0p001_dzero | 0.0000 | 0.0000 | +0.00% | no |
| g_baseline_-h_a0p0001_dzero | 0.0000 | 0.0000 | -0.03% | no |
| g_baseline_xh_aoff_dzero | 0.0000 | 0.0000 | -0.03% | no |
| g_baseline_xh_a0p01_dzero | 0.0000 | 0.0000 | -0.03% | no |
| g_baseline_xh_a0p0001_dnone | 0.0000 | 0.0000 | -0.06% | no |
| g_baseline_xh_a0p01_dnone | 0.0000 | 0.0000 | -0.06% | no |
| g_baseline_-h_a0p01_dzero | 0.0000 | 0.0000 | -0.09% | no |
| g_baseline_-h_aoff_dzero | 0.0000 | 0.0000 | -0.12% | no |
| g_baseline_-h_a0p001_dzero | 0.0000 | 0.0000 | -0.18% | no |

## Predicted but not measured (within budget)

4 genomes; 120 predicted over budget; 0 failed to build.
