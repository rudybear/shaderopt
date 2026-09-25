# bloom_hdr_edges: graph-level experiments (formats, resolution)

Whole-chain GPU time vs the unmodified chain, interleaved; quality on the chain's judged outputs. These are proposals for the app, never applied automatically.

| experiment | kind | pass | from | to | chain baseline us | chain variant us | speedup | 95% CI | FLIP mean / p99 (final) | within budget | timing gate | verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| fmt_threshold_R11G11B10F | format | threshold | RGBA16F | R11G11B10F | 23.88 | 23.65 | +0.97% | [+0.82%, +1.07%] | 0.0018 / 0.0203 | yes | no | within budget, no speedup |
| fmt_threshold_RGBA8 | format | threshold | RGBA16F | RGBA8 | 23.88 | 23.65 | +0.99% | [+0.90%, +1.10%] | 0.1280 / 0.9712 | no | no | rejected |
| res_threshold_0.25 | resolution | threshold | 0.5 | 0.25 | 23.88 | 21.85 | +8.53% | [+8.41%, +8.64%] | 0.0493 / 0.5593 | no | no | rejected |
| fmt_blur_h_R11G11B10F | format | blur_h | RGBA16F | R11G11B10F | 23.89 | 23.53 | +1.50% | [+1.36%, +1.61%] | 0.0021 / 0.0221 | yes | no | within budget, no speedup |
| fmt_blur_h_RGBA8 | format | blur_h | RGBA16F | RGBA8 | 23.88 | 23.51 | +1.54% | [+1.41%, +1.63%] | 0.1000 / 0.8998 | no | no | rejected |
| res_blur_h_0.25 | resolution | blur_h | 0.5 | 0.25 | 23.87 | 22.22 | +6.91% | [+6.79%, +7.03%] | 0.0349 / 0.3907 | no | no | rejected |
| fmt_blur_v_R11G11B10F | format | blur_v | RGBA16F | R11G11B10F | 23.89 | 23.45 | +1.84% | [+1.72%, +1.94%] | 0.0023 / 0.0236 | yes | no | within budget, no speedup |
| fmt_blur_v_RGBA8 | format | blur_v | RGBA16F | RGBA8 | 23.87 | 23.44 | +1.78% | [+1.66%, +1.88%] | 0.0559 / 0.5980 | no | no | rejected |
| res_blur_v_0.25 | resolution | blur_v | 0.5 | 0.25 | 23.88 | 22.05 | +7.70% | [+7.61%, +7.78%] | 0.0276 / 0.3262 | no | no | rejected |
