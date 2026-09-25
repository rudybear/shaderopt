# bloom_hdr_gradient: graph-level experiments (formats, resolution)

Whole-chain GPU time vs the unmodified chain, interleaved; quality on the chain's judged outputs. These are proposals for the app, never applied automatically.

| experiment | kind | pass | from | to | chain baseline us | chain variant us | speedup | 95% CI | FLIP mean / p99 (final) | within budget | timing gate | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| fmt_threshold_R11G11B10F | format | threshold | RGBA16F | R11G11B10F | 23.87 | 23.63 | +1.01% | [+0.91%, +1.10%] | 0.0000 / 0.0000 | yes | no | within budget, no speedup |
| fmt_threshold_RGBA8 | format | threshold | RGBA16F | RGBA8 | 23.90 | 23.65 | +1.03% | [+0.91%, +1.14%] | 0.0000 / 0.0000 | yes | no | within budget, no speedup |
| res_threshold_0.25 | resolution | threshold | 0.5 | 0.25 | 23.87 | 21.83 | +8.55% | [+8.41%, +8.66%] | 0.0000 / 0.0000 | yes | no | within budget, no speedup |
| fmt_blur_h_R11G11B10F | format | blur_h | RGBA16F | R11G11B10F | 23.93 | 23.53 | +1.66% | [+1.53%, +1.78%] | 0.0000 / 0.0000 | yes | no | within budget, no speedup |
| fmt_blur_h_RGBA8 | format | blur_h | RGBA16F | RGBA8 | 23.91 | 23.57 | +1.44% | [+1.32%, +1.58%] | 0.0000 / 0.0000 | yes | no | within budget, no speedup |
| res_blur_h_0.25 | resolution | blur_h | 0.5 | 0.25 | 23.93 | 22.25 | +7.03% | [+6.92%, +7.11%] | 0.0000 / 0.0000 | yes | no | within budget, no speedup |
| fmt_blur_v_R11G11B10F | format | blur_v | RGBA16F | R11G11B10F | 23.93 | 23.52 | +1.74% | [+1.61%, +1.81%] | 0.0000 / 0.0000 | yes | no | within budget, no speedup |
| fmt_blur_v_RGBA8 | format | blur_v | RGBA16F | RGBA8 | 23.94 | 23.53 | +1.70% | [+1.60%, +1.80%] | 0.0000 / 0.0000 | yes | no | within budget, no speedup |
| res_blur_v_0.25 | resolution | blur_v | 0.5 | 0.25 | 23.94 | 22.14 | +7.53% | [+7.42%, +7.67%] | 0.0000 / 0.0000 | yes | no | within budget, no speedup |
