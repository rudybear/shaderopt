# gaussian_blur_h: M5 search

Genome = (source, exact, hoist, approx level, demote level, mode). 144 genomes enumerated and predicted in the CPU model against the ORIGINAL shader; 12 measured on the device (budget 8 + neighbours). Tolerance: {'metric': 'flip_hdr', 'mean_max': 0.01, 'p99_max': 0.05, 'min_speedup': 0.1}.

## Measured (Pareto on measured speedup vs measured error)

| genome | predicted p99 | measured worst p99 | min speedup (train) | accepted |
|---|---|---|---|---|
| g_bilinear5_--_a0p001_dnone | 0.0071 | 0.0071 | +11.17% | yes |
| g_bilinear5_--_a0p01_dnone | 0.0071 | 0.0071 | +11.10% | yes |
| g_bilinear5_--_a0p001_dall | 0.0071 | 0.9674 | +10.97% | no |
| g_bilinear5_--_a0p01_dq2 | 0.0071 | 0.9674 | +10.89% | no |
| g_bilinear5_--_a0p001_dq4 | 0.0071 | 0.9674 | +10.87% | no |
| g_bilinear5_--_a0p001_dq2 | 0.0071 | 0.9674 | +10.86% | no |
| g_bilinear5_--_a0p01_dall | 0.0071 | 0.9674 | +10.85% | no |
| g_bilinear5_--_a0p001_dq1 | 0.0071 | 0.9674 | +10.79% | no |
| g_bilinear5_--_a0p01_dq4 | 0.0071 | 0.9674 | +10.78% | no |
| g_bilinear5_--_a0p001_dzero | 0.0071 | 0.9674 | +10.70% | no |
| g_bilinear5_--_a0p01_dq1 | 0.0071 | 0.9674 | +10.30% | no |
| g_baseline_--_a0p01_dq2 | 0.0073 | 0.9674 | +3.08% | no |

## Predicted but not measured (within budget)

82 genomes; 0 predicted over budget; 48 failed to build.
