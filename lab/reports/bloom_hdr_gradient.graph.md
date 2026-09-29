# bloom_hdr_gradient: graph-level experiments (formats, resolution)

Whole-chain GPU time vs the unmodified chain, interleaved; quality on the chain's judged outputs. These are proposals for the app, never applied automatically.

| experiment | kind | pass | from | to | chain baseline us | chain variant us | speedup | 95% CI | FLIP mean / p99 (final) | within budget | timing gate | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| fmt_threshold_R11G11B10F | format | threshold | RGBA16F | R11G11B10F | 298.47 | 293.77 | +1.58% | [+0.51%, +3.21%] | 0.0000 / 0.0000 | yes | no | within budget, no speedup |
| fmt_threshold_RGBA8 | format | threshold | RGBA16F | RGBA8 | 290.35 | 291.59 | -0.43% | [-1.23%, +0.14%] | 0.0000 / 0.0000 | yes | no | within budget, no speedup |
| res_threshold_0.25 | resolution | threshold | 0.5 | 0.25 | 294.85 | 289.98 | +1.65% | [+1.27%, +2.17%] | 0.0000 / 0.0000 | yes | no | within budget, no speedup |
| fmt_blur_h_R11G11B10F | format | blur_h | RGBA16F | R11G11B10F | 294.76 | 289.04 | +1.94% | [+1.33%, +2.35%] | 0.0000 / 0.0000 | yes | no | within budget, no speedup |
| fmt_blur_h_RGBA8 | format | blur_h | RGBA16F | RGBA8 | 294.58 | 290.14 | +1.51% | [+1.16%, +1.89%] | 0.0000 / 0.0000 | yes | no | within budget, no speedup |
| res_blur_h_0.25 | resolution | blur_h | 0.5 | 0.25 | 300.37 | 317.70 | -5.77% | [-8.35%, -3.01%] | 0.0000 / 0.0000 | yes | no | within budget, no speedup |
| fmt_blur_v_R11G11B10F | format | blur_v | RGBA16F | R11G11B10F | 302.90 | 324.69 | -7.19% | [-32.51%, +2.71%] | 0.0000 / 0.0000 | yes | no | within budget, no speedup |
| fmt_blur_v_RGBA8 | format | blur_v | RGBA16F | RGBA8 | 292.24 | 303.84 | -3.97% | [-5.91%, -2.19%] | 0.0000 / 0.0000 | yes | no | within budget, no speedup |
| res_blur_v_0.25 | resolution | blur_v | 0.5 | 0.25 | 322.41 | 344.66 | -6.90% | [-28.85%, +14.90%] | 0.0000 / 0.0000 | yes | no | within budget, no speedup |
