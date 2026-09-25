# gaussian_blur_h: M5 search

Genome = (source, exact, hoist, approx level, demote level, mode). 144 genomes enumerated and predicted in the CPU model against the ORIGINAL shader; 12 measured on the device (budget 8 + neighbours). Tolerance: {'metric': 'flip_hdr', 'mean_max': 0.01, 'p99_max': 0.05, 'min_speedup': 0.1}.

## Measured (Pareto on measured speedup vs measured error)

| genome | predicted p99 | measured worst p99 | min speedup (train) | accepted |
|---|---|---|---|---|
| g_bilinear5_xh_a0p001_dq4 | 0.0071 | 0.9674 | +11.20% | no |
| g_bilinear5_xh_a0p001_dnone | 0.0071 | 0.0071 | +11.06% | yes |
| g_bilinear5_xh_a0p001_dq1 | 0.0071 | 0.9674 | +11.02% | no |
| g_bilinear5_xh_a0p001_dq2 | 0.0071 | 0.9674 | +11.01% | no |
| g_bilinear5_xh_a0p01_dq1 | 0.0071 | 0.9674 | +11.00% | no |
| g_bilinear5_xh_a0p01_dall | 0.0071 | 0.9674 | +10.96% | no |
| g_bilinear5_xh_a0p01_dq2 | 0.0071 | 0.9674 | +10.93% | no |
| g_bilinear5_xh_a0p0001_dq1 | 0.0071 | 0.9674 | +10.90% | no |
| g_bilinear5_xh_a0p001_dall | 0.0071 | 0.9674 | +10.87% | no |
| g_bilinear5_--_a0p001_dq2 | 0.0071 | 0.9674 | +10.86% | no |
| g_bilinear5_xh_a0p01_dq4 | 0.0071 | 0.9674 | +10.74% | no |
| g_baseline_xh_a0p001_dq1 | 0.0019 | 0.9674 | +3.30% | no |

## Predicted but not measured (within budget)

130 genomes; 0 predicted over budget; 0 failed to build.
