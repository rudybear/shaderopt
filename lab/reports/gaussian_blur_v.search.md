# gaussian_blur_v: M5 search

Genome = (source, exact, hoist, approx level, demote level, mode). 144 genomes enumerated and predicted in the CPU model against the ORIGINAL shader; 12 measured on the device (budget 8 + neighbours). Tolerance: {'metric': 'flip_hdr', 'mean_max': 0.01, 'p99_max': 0.05, 'min_speedup': 0.1}.

## Measured (Pareto on measured speedup vs measured error)

| genome | predicted p99 | measured worst p99 | min speedup (train) | accepted |
|---|---|---|---|---|
| g_bilinear5_xh_a0p01_dall | 0.0102 | 0.9674 | +11.54% | no |
| g_bilinear5_xh_a0p01_dnone | 0.0103 | 0.0100 | +11.53% | yes |
| g_bilinear5_xh_a0p01_dq1 | 0.0102 | 0.9674 | +11.49% | no |
| g_bilinear5_xh_a0p01_dq2 | 0.0102 | 0.9674 | +11.46% | no |
| g_bilinear5_xh_a0p001_dq1 | 0.0102 | 0.9674 | +11.46% | no |
| g_bilinear5_xh_a0p001_dall | 0.0102 | 0.9674 | +11.42% | no |
| g_bilinear5_xh_a0p001_dq4 | 0.0102 | 0.9674 | +11.42% | no |
| g_bilinear5_xh_a0p001_dq2 | 0.0102 | 0.9674 | +11.42% | no |
| g_bilinear5_xh_a0p01_dq4 | 0.0102 | 0.9674 | +11.39% | no |
| g_bilinear5_xh_a0p0001_dq1 | 0.0102 | 0.9674 | +11.16% | no |
| g_bilinear5_--_a0p01_dq2 | 0.0102 | 0.9674 | +10.90% | no |
| g_baseline_xh_a0p01_dq1 | 0.0023 | 0.9674 | +4.14% | no |

## Predicted but not measured (within budget)

130 genomes; 0 predicted over budget; 0 failed to build.
