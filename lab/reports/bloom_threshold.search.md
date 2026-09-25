# bloom_threshold: M5 search

Genome = (source, exact, hoist, approx level, demote level, mode). 72 genomes enumerated and predicted in the CPU model against the ORIGINAL shader; 11 measured on the device (budget 8 + neighbours). Tolerance: {'metric': 'flip_hdr', 'mean_max': 0.01, 'p99_max': 0.05, 'min_speedup': 0.1}.

## Measured (Pareto on measured speedup vs measured error)

| genome | predicted p99 | measured worst p99 | min speedup (train) | accepted |
|---|---|---|---|---|
| g_baseline_xh_a0p01_dq1 | 0.0038 | 0.9674 | +0.06% | no |
| g_baseline_xh_a0p001_dnone | 0.0000 | 0.0000 | +0.06% | no |
| g_baseline_xh_a0p001_dq4 | 0.0038 | 0.9674 | +0.03% | no |
| g_baseline_xh_a0p001_dall | 0.0038 | 0.9674 | +0.00% | no |
| g_baseline_xh_a0p01_dall | 0.0038 | 0.9674 | -0.03% | no |
| g_baseline_xh_a0p01_dq2 | 0.0038 | 0.9674 | -0.06% | no |
| g_baseline_xh_a0p01_dq4 | 0.0038 | 0.9674 | -0.06% | no |
| g_baseline_xh_a0p001_dq2 | 0.0038 | 0.9674 | -0.09% | no |
| g_baseline_xh_a0p001_dq1 | 0.0038 | 0.9674 | -0.09% | no |
| g_baseline_--_a0p001_dq4 | 0.0038 | 0.9674 | -0.31% | no |
| g_baseline_xh_a0p01_dzero | 0.0052 | 0.9674 | -0.49% | no |

## Predicted but not measured (within budget)

57 genomes; 0 predicted over budget; 0 failed to build.
