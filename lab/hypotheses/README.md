# Semantic rewrite hypotheses (the AI step)

Each subdirectory `<shader>/<hypothesis-id>/` holds `<shader>.frag` (a full replacement source with the same interface) and `hypothesis.toml`:

```toml
[hypothesis]
shader = "gaussian_blur_h"
id = "bilinear5"
claim = "..."                # the algorithmic claim, in words
class = "semantic"           # semantic (changes what is computed) | ulp (identical in real arithmetic)
predicted_cost = "9 -> 5 texture fetches, 9 -> 5 fma"
predicted_error = "sub-code on smooth inputs; up to 1 code on hard edges (bilinear weights are quantized to 8 bits)"
exposes = ["hdr_edges", "noise"]  # scenarios most likely to expose the error
proposed_by = "Claude Fable 5.1, 2026-09-25, from the M2 classification report"
```

These are hypotheses, never evidence. `lab hypo` compiles each with the pinned glslang, validates, predicts quality in the CPU model against the baseline model, then measures on the device interleaved with the baseline and applies the gates. Outcomes, positive and negative, are logged in `lab/results.jsonl` and the per-shader report.
