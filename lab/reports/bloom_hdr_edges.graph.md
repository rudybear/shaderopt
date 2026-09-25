# bloom_hdr_edges: graph-level experiments (formats, resolution)

Whole-chain GPU time vs the unmodified chain, interleaved; quality on the chain's judged outputs. These are proposals for the app, never applied automatically.

| experiment | kind | pass | from | to | chain baseline us | chain variant us | speedup | 95% CI | FLIP mean / p99 (final) | within budget | timing gate | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| fmt_threshold_R11G11B10F | format | threshold | RGBA16F | R11G11B10F | 23.86 | 23.88 | -0.10% | [-0.21%, +0.02%] | 0.0000 / 0.0000 | yes | no | within budget, no speedup |
| fmt_threshold_RGBA8 | format | threshold | RGBA16F | RGBA8 | 23.90 | 23.90 | -0.03% | [-0.12%, +0.09%] | 0.0000 / 0.0000 | yes | no | within budget, no speedup |
| res_threshold_0.25 | resolution | threshold | 0.5 | 0.25 | 23.90 | 23.88 | +0.08% | [-0.02%, +0.22%] | 0.0000 / 0.0000 | yes | no | within budget, no speedup |
| fmt_blur_h_R11G11B10F | format | blur_h | RGBA16F | R11G11B10F | 23.89 | 23.90 | -0.02% | [-0.19%, +0.08%] | 0.0000 / 0.0000 | yes | no | within budget, no speedup |
| fmt_blur_h_RGBA8 | format | blur_h | RGBA16F | RGBA8 | 23.89 | 23.89 | +0.03% | [-0.07%, +0.14%] | 0.0000 / 0.0000 | yes | no | within budget, no speedup |
| res_blur_h_0.25 | resolution | blur_h | 0.5 | 0.25 | 23.90 | 23.89 | +0.08% | [-0.08%, +0.14%] | 0.0000 / 0.0000 | yes | no | within budget, no speedup |
| fmt_blur_v_R11G11B10F | format | blur_v | RGBA16F | R11G11B10F | 23.88 | 23.89 | -0.06% | [-0.15%, +0.03%] | 0.0000 / 0.0000 | yes | no | within budget, no speedup |
| fmt_blur_v_RGBA8 | format | blur_v | RGBA16F | RGBA8 | 23.91 | 23.90 | +0.04% | [-0.08%, +0.14%] | 0.0000 / 0.0000 | yes | no | within budget, no speedup |
| res_blur_v_0.25 | resolution | blur_v | 0.5 | 0.25 | 23.91 | 23.92 | -0.02% | [-0.13%, +0.18%] | 0.0000 / 0.0000 | yes | no | within budget, no speedup |
