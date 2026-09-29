# gaussian_blur_h: M5 search

Genome = (source, exact, hoist, approx level, demote level, mode). 144 genomes enumerated and predicted in the CPU model against the ORIGINAL shader; 9 measured on the device (budget 6 + neighbours). Tolerance: {'metric': 'flip_hdr', 'mean_max': 0.01, 'p99_max': 0.05, 'min_speedup': 0.1}.

## Measured (Pareto on measured speedup vs measured error)

| genome | predicted p99 | measured worst p99 | min speedup (train) | accepted |
|---|---|---|---|---|
| g_bilinear5_xh_a0p01_dall | 0.0073 | 0.0110 | +1.18% | no |
| g_bilinear5_xh_a0p01_dq1 | 0.0073 | 0.0110 | +1.09% | no |
| g_bilinear5_xh_a0p001_dq2 | 0.0073 | 0.0110 | +0.87% | no |
| g_bilinear5_--_a0p001_dq2 | 0.0073 | 0.0110 | +0.77% | no |
| g_bilinear5_xh_aoff_dq2 | 0.0073 | 0.0110 | +0.62% | no |
| g_bilinear5_xh_a0p01_dq4 | 0.0073 | 0.0110 | +0.21% | no |
| g_bilinear5_xh_a0p001_dq1 | 0.0073 | 0.0110 | +0.07% | no |
| g_bilinear5_xh_a0p001_dq4 | 0.0073 | 0.0110 | -0.62% | no |
| g_bilinear5_xh_a0p01_dq2 | 0.0073 | 0.0110 | -2.83% | no |

## Predicted but not measured (within budget)

133 genomes; 0 predicted over budget; 0 failed to build.
