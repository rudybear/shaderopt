# color_grade: M5 search

Genome = (source, exact, hoist, approx level, demote level, mode). 144 genomes enumerated and predicted in the CPU model against the ORIGINAL shader; 9 measured on the device (budget 6 + neighbours). Tolerance: {'metric': 'flip', 'mean_max': 0.01, 'p99_max': 0.05, 'min_speedup': 0.1}.

## Measured (Pareto on measured speedup vs measured error)

| genome | predicted p99 | measured worst p99 | min speedup (train) | accepted |
|---|---|---|---|---|
| g_foldmat_-h_a0p01_dq4 | 0.0331 | 0.0319 | +8.05% | no |
| g_foldmat_-h_a0p001_dq4 | 0.0331 | 0.0319 | +6.85% | no |
| g_foldmat_xh_a0p01_dnone | 0.0000 | 0.0000 | -1.23% | no |
| g_foldmat_xh_a0p01_dq4 | 0.0331 | 0.0319 | -3.10% | no |
| g_foldmat_xh_a0p001_dq4 | 0.0331 | 0.0319 | -3.25% | no |
| g_foldmat_-h_a0p01_dzero | 0.0000 | 0.0000 | -12.16% | no |
| g_baseline_-h_a0p001_dq4 | 0.0324 | 0.0308 | -12.57% | no |
| g_foldmat_xh_a0p0001_dq4 | 0.0331 | 0.0319 | -13.44% | no |
| g_foldmat_-h_a0p0001_dq4 | 0.0331 | 0.0319 | -16.83% | no |

## Predicted but not measured (within budget)

55 genomes; 72 predicted over budget; 0 failed to build.
