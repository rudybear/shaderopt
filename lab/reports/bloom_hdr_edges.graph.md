# bloom_hdr_edges: graph-level experiments (formats, resolution)

Whole-chain GPU time vs the unmodified chain, interleaved; quality on the chain's judged outputs. These are proposals for the app, never applied automatically.

| experiment | kind | pass | from | to | chain baseline us | chain variant us | speedup | 95% CI | FLIP mean / p99 (final) | within budget | timing gate | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| fmt_threshold_R11G11B10F | format | threshold | RGBA16F | R11G11B10F | 256.65 | 278.98 | -8.70% | [-20.91%, -0.87%] | 0.0015 / 0.0200 | yes | no | within budget, no speedup |
| fmt_threshold_RGBA8 | format | threshold | RGBA16F | RGBA8 | 291.63 | 263.71 | +9.57% | [-2.00%, +26.77%] | 0.1283 / 0.9713 | no | no | rejected |
| res_threshold_0.25 | resolution | threshold | 0.5 | 0.25 | 259.02 | 262.18 | -1.22% | [-2.01%, -0.44%] | 0.0494 / 0.5595 | no | no | rejected |
| fmt_blur_h_R11G11B10F | format | blur_h | RGBA16F | R11G11B10F | 262.59 | 298.54 | -13.69% | [-33.18%, +0.03%] | 0.0023 / 0.0229 | yes | no | within budget, no speedup |
| fmt_blur_h_RGBA8 | format | blur_h | RGBA16F | RGBA8 | 288.35 | 293.24 | -1.70% | [-34.82%, +28.39%] | 0.1002 / 0.9009 | no | no | rejected |
| res_blur_h_0.25 | resolution | blur_h | 0.5 | 0.25 | 256.48 | 251.40 | +1.98% | [-2.44%, +3.01%] | 0.0349 / 0.3909 | no | no | rejected |
| fmt_blur_v_R11G11B10F | format | blur_v | RGBA16F | R11G11B10F | 254.64 | 253.21 | +0.56% | [-3.43%, +1.71%] | 0.0024 / 0.0237 | yes | no | within budget, no speedup |
| fmt_blur_v_RGBA8 | format | blur_v | RGBA16F | RGBA8 | 257.66 | 253.74 | +1.52% | [+1.18%, +1.78%] | 0.0558 / 0.5981 | no | no | rejected |
| res_blur_v_0.25 | resolution | blur_v | 0.5 | 0.25 | 255.47 | 260.81 | -2.09% | [-2.77%, -1.56%] | 0.0275 / 0.3265 | no | no | rejected |
