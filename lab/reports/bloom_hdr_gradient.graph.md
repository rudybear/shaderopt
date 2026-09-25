# bloom_hdr_gradient: graph-level experiments (formats, resolution)

Whole-chain GPU time vs the unmodified chain, interleaved; quality on the chain's judged outputs. These are proposals for the app, never applied automatically.

| experiment | kind | pass | from | to | chain baseline us | chain variant us | speedup | 95% CI | FLIP mean / p99 (final) | within budget | timing gate | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| fmt_threshold_R11G11B10F | format | threshold | RGBA16F | R11G11B10F | 23.93 | 23.92 | +0.01% | [-0.08%, +0.13%] | 0.0000 / 0.0000 | yes | no | within budget, no speedup |
| fmt_threshold_RGBA8 | format | threshold | RGBA16F | RGBA8 | 23.93 | 23.95 | -0.08% | [-0.20%, +0.03%] | 0.0000 / 0.0000 | yes | no | within budget, no speedup |
| res_threshold_0.25 | resolution | threshold | 0.5 | 0.25 | 23.92 | 23.93 | -0.03% | [-0.10%, +0.10%] | 0.0000 / 0.0000 | yes | no | within budget, no speedup |
| fmt_blur_h_R11G11B10F | format | blur_h | RGBA16F | R11G11B10F | 23.93 | 23.94 | -0.02% | [-0.17%, +0.11%] | 0.0000 / 0.0000 | yes | no | within budget, no speedup |
| fmt_blur_h_RGBA8 | format | blur_h | RGBA16F | RGBA8 | 23.93 | 23.93 | +0.03% | [-0.11%, +0.16%] | 0.0000 / 0.0000 | yes | no | within budget, no speedup |
| res_blur_h_0.25 | resolution | blur_h | 0.5 | 0.25 | 23.94 | 23.95 | -0.04% | [-0.15%, +0.06%] | 0.0000 / 0.0000 | yes | no | within budget, no speedup |
| fmt_blur_v_R11G11B10F | format | blur_v | RGBA16F | R11G11B10F | 23.93 | 23.93 | -0.03% | [-0.14%, +0.09%] | 0.0000 / 0.0000 | yes | no | within budget, no speedup |
| fmt_blur_v_RGBA8 | format | blur_v | RGBA16F | RGBA8 | 23.94 | 23.94 | -0.01% | [-0.08%, +0.10%] | 0.0000 / 0.0000 | yes | no | within budget, no speedup |
| res_blur_v_0.25 | resolution | blur_v | 0.5 | 0.25 | 23.93 | 23.93 | +0.02% | [-0.07%, +0.15%] | 0.0000 / 0.0000 | yes | no | within budget, no speedup |
