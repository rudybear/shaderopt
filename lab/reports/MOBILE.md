# Mobile report: Samsung Galaxy Tab S10 Ultra (Mali-G720 Immortalis MC12)

Date: 2026-09-28. Device: SM-X920, MediaTek Dimensity 9300, Mali-G720-Immortalis MC12, driver v1.r44p1, Vulkan 1.3.247, Android 16, timestamp period 76.9 ns, GPU clock not readable (SELinux). Protocol: headless runner via adb from `/data/local/tmp`, 3 command buffers in flight, 32 executions per submit, 20 warmup submits, 15 samples per job, 4 interleaved B/V rounds on every scenario, plateau filter on samples, round-paired speedup estimator for gate 4, thermal gate at SEVERE (never triggered; status stayed 0). Inputs in app-realistic formats (RGBA16F HDR, RGBA8 LDR, R16F shadow map). A Pixel 9 Pro XL (Mali-G715) was probed first and behaved the same way but was taken back before its sweep.

## Noise floor (A/A)
Plateau A/A differences 0.3..1.5%; coefficient of variation 1.2..1.5% on ALU-bound passes, 6..11% on bandwidth-bound full-resolution passes (bursts 30..60% above a stable floor that no readable counter explains). Gate 4 therefore requires max(2%, floor, 10% budget minimum) with a paired CI excluding zero; several strong effects below fail on the CI, not on size.

## CPU model on a second vendor
`lab lift-check` on the Mali: bloom chain bit-exact, tonemap and fxaa within one code unit. The 8-bit round-to-nearest sampler model and the code-unit tolerances transfer unchanged from NVIDIA to Arm.

## Baselines (median, 1080p)
threshold 38..81 us, blur 56..60 us, composite 101..111 us, tonemap 150..163 us, color_grade 92..154 us, fxaa 110..116 us, deferred_lit 138 us, vignette 80 us. Roughly 10..20x the RTX PRO 6000.

## Verdicts

| shader | variant | tablet | desktop | verdict on the tablet |
|---|---|---|---|---|
| deferred_lit | pcf4 (AI hypothesis) | **+24.1%**, p99 0.027 | +20.7% | **ACCEPTED** |
| deferred_lit | search: pcf4 + exact + hoist + f16 demotion | +21..23%, p99 0.039 | +14% | accepted, but no better than pcf4 alone |
| gaussian_blur_h/v | bilinear5 | +19..31% on edges/gradient/extreme, **+1..2% on noise** | +11% everywhere | not accepted: the noise scene fails the timing gate (incoherent texture access makes 5 bilinear fetches cost as much as 9 on this GPU) |
| tonemap_aces | explicit f16, 32 safe sites (q2) | +12.5..42% train, quality 5/5, timing 3/5 | +1.9% | strong but not accepted: two holdout scenes miss the CI; the brief's mobile prediction is confirmed in direction |
| tonemap_aces | RelaxedPrecision, same sites | +36..39% on two scenes, negative on others | no effect | inconsistent across scenes; the driver honours the hint but the measurement is not stable enough to accept |
| tonemap_aces | 1D LUT (AI hypothesis) | +4..11%, **fails NaN/Inf holdout (p99 0.97)** | 60% slower | platform-specific win once a NaN guard is added; rejected as written |
| tonemap_aces | onepow (fma form) | -14..+58%, CI spans zero | no effect | inconsistent, needs repeat |
| color_grade | foldmat (CPU-folded matrix) | -3.5..+2.9% | +1.5% | no effect |
| color_grade | foldmat + f16 demotion (search) | +6.9..8.1%, p99 0.032 | | below the 10% budget minimum |
| fxaa | dir4 | +2.6..10.4%, p99 0.27..0.29 | same | quality-rejected on both platforms |
| hoisting (all shaders) | exact | -2.5..+3% | <= +1.5% | no effect; uniform ALU is free here too |
| formats: R11G11B10F bloom intermediates | | -14..+2% chain, within budget | +1..2% | inconsistent; bandwidth-state noise |
| resolution: quarter-res bloom | | -7..+2% chain | +7..8.5% | **slower on the tiler** (extra passes at another resolution cost more than the bandwidth saved) |
| vignette cheaphash | | -15..+5.6%, p99 0.34..0.46 | quality-rejected | same |

## What the mobile pass established
1. **Platform-specific verdicts exist and the pipeline catches them:** the 1D LUT tonemapper is 60% slower on the desktop and 10% faster on the Mali; quarter-resolution bloom is +8% on desktop and slower on the tiler; explicit f16 and RelaxedPrecision, worth nothing on desktop, move the tonemapper by tens of percent on the Mali.
2. **Algorithmic rewrites transfer:** 2x2 PCF is accepted on both device classes; the bilinear blur is accepted on desktop and on three of four tablet scenes.
3. **The limiting factor on mobile is measurement noise, not effect size.** Without a lockable clock, a readable memory-bus state or sustained-performance mode, the paired estimator needs more rounds than the 4 used here for effects under ~10%. The brief's next step applies: confirm finalists in the foreground APK with sustained performance mode.
4. **Holdout scenarios keep doing their job:** NaN/Inf inputs reject the LUT and gate f16 sets; the noise scene rejects the bilinear blur here.

## Open items
- Repeat the tonemap f16 and RelaxedPrecision sets with 8+ rounds; if the CI holds, they are the first accepted precision demotions.
- Add a NaN/Inf guard hypothesis for the LUT tonemapper (`max(c, 0)` and a finite clamp before `log2`).
- Build the IGL shell APK path for sustained performance mode and foreground confirmation (gate 5).
- Pixel 9 Pro XL sweep when the phone is available again (its A/A floor was 0.2..0.4% on chain passes once the runner kept it busy).
