# gaussian_blur_h: M5 search

Genome = (source, exact, hoist, approx level, demote level, mode). 144 genomes enumerated and predicted in the CPU model against the ORIGINAL shader; 11 measured on the device (budget 8 + neighbours). Tolerance: {'metric': 'flip_hdr', 'mean_max': 0.01, 'p99_max': 0.05, 'min_speedup': 0.1}.

## Measured (Pareto on measured speedup vs measured error)

| genome | predicted p99 | measured worst p99 | min speedup (train) | accepted |
|---|---|---|---|---|
| g_bilinear5_-h_a0p0001_dnone | 0.0071 | 0.0071 | +11.19% | yes |
| g_bilinear5_xh_a0p001_dnone | 0.0071 | 0.0071 | +11.17% | yes |
| g_bilinear5_--_a0p0001_dnone | 0.0071 | 0.0071 | +11.04% | yes |
| g_bilinear5_xh_a0p0001_dnone | 0.0071 | 0.0071 | +11.04% | yes |
| g_bilinear5_-h_a0p001_dnone | 0.0071 | 0.0071 | +11.04% | yes |
| g_bilinear5_-h_aoff_dnone | 0.0071 | 0.0071 | +11.00% | yes |
| g_bilinear5_--_a0p001_dnone | 0.0071 | 0.0071 | +11.00% | yes |
| g_bilinear5_xh_aoff_dnone | 0.0071 | 0.0071 | +11.00% | yes |
| g_bilinear5_xh_a0p01_dnone | 0.0071 | 0.0071 | +10.94% | yes |
| g_bilinear5_-h_a0p01_dnone | 0.0071 | 0.0071 | +10.92% | yes |
| g_baseline_-h_a0p0001_dnone | 0.0011 | 0.0052 | -0.24% | no |

## Predicted but not measured (within budget)

11 genomes; 120 predicted over budget; 0 failed to build.
