# gaussian_blur_v: M5 search

Genome = (source, exact, hoist, approx level, demote level, mode). 144 genomes enumerated and predicted in the CPU model against the ORIGINAL shader; 12 measured on the device (budget 8 + neighbours). Tolerance: {'metric': 'flip_hdr', 'mean_max': 0.01, 'p99_max': 0.05, 'min_speedup': 0.1}.

## Measured (Pareto on measured speedup vs measured error)

| genome | predicted p99 | measured worst p99 | min speedup (train) | accepted |
|---|---|---|---|---|
| g_bilinear5_-h_aoff_dnone | 0.0103 | 0.0100 | +11.62% | yes |
| g_bilinear5_xh_a0p0001_dnone | 0.0103 | 0.0100 | +11.55% | yes |
| g_bilinear5_-h_a0p0001_dnone | 0.0103 | 0.0100 | +11.54% | yes |
| g_bilinear5_xh_a0p01_dnone | 0.0103 | 0.0100 | +11.50% | yes |
| g_bilinear5_-h_a0p01_dnone | 0.0103 | 0.0100 | +11.48% | yes |
| g_bilinear5_-h_a0p001_dnone | 0.0103 | 0.0100 | +11.36% | yes |
| g_bilinear5_xh_aoff_dnone | 0.0103 | 0.0100 | +11.35% | yes |
| g_bilinear5_xh_a0p001_dnone | 0.0103 | 0.0100 | +11.16% | yes |
| g_bilinear5_--_aoff_dnone | 0.0103 | 0.0100 | +11.15% | yes |
| g_bilinear5_--_a0p0001_dnone | 0.0103 | 0.0100 | +11.13% | yes |
| g_baseline_xh_a0p0001_dnone | 0.0000 | 0.0000 | +0.12% | no |
| g_baseline_-h_a0p0001_dnone | 0.0011 | 0.0019 | -0.32% | no |

## Predicted but not measured (within budget)

10 genomes; 120 predicted over budget; 0 failed to build.
