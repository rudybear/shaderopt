# color_grade: M5 search

Genome = (source, exact, hoist, approx level, demote level, mode). 144 genomes enumerated and predicted in the CPU model against the ORIGINAL shader; 12 measured on the device (budget 8 + neighbours). Tolerance: {'metric': 'flip', 'mean_max': 0.01, 'p99_max': 0.05, 'min_speedup': 0.1}.

## Measured (Pareto on measured speedup vs measured error)

| genome | predicted p99 | measured worst p99 | min speedup (train) | accepted |
|---|---|---|---|---|
| g_foldmat_xh_a0p01_dzero | 0.0000 | 0.0000 | +1.15% | no |
| g_foldmat_-h_a0p001_dnone | 0.0000 | 0.0000 | +1.09% | no |
| g_foldmat_xh_a0p001_dnone | 0.0000 | 0.0000 | +1.06% | no |
| g_foldmat_xh_aoff_dzero | 0.0000 | 0.0000 | +1.06% | no |
| g_baseline_xh_a0p001_dzero | 0.0000 | 0.0000 | +0.99% | no |
| g_foldmat_xh_a0p001_dzero | 0.0000 | 0.0000 | +0.97% | no |
| g_foldmat_-h_a0p001_dq4 | 0.0164 | 0.0158 | -0.48% | no |
| g_foldmat_-h_a0p0001_dq4 | 0.0164 | 0.0158 | -1.02% | no |
| g_foldmat_-h_a0p01_dq4 | 0.0164 | 0.0158 | -1.07% | no |
| g_foldmat_xh_a0p0001_dq4 | 0.0164 | 0.0158 | -1.13% | no |
| g_foldmat_xh_a0p01_dq4 | 0.0164 | 0.0158 | -1.16% | no |
| g_foldmat_xh_a0p001_dq4 | 0.0164 | 0.0158 | -1.19% | no |

## Predicted but not measured (within budget)

52 genomes; 72 predicted over budget; 0 failed to build.
