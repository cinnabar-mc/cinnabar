# Android terrain draw path on Adreno 650

On a Galaxy S20 FE 5G, the batched terrain unit path sustains 20% more frames per second on
opaque terrain than the per-section fallback after the ordering fix in
[#3070](https://github.com/cinnabar-mc/cinnabar/pull/3070) (`08446aa3ae`): 26.0 against 21.6
frames per second, and a median GPU frame time 16% shorter, 38.3 against 45.4 ms, with both
paths drawing 296 sections. Before #3070 the same comparison, run as separate processes, put
the per-section path ahead. Disabling batching on proprietary Adreno Vulkan is therefore a
regression after #3070, not a workaround.

Vanilla Bedrock 1.26.60 measures 113.1 fps at a comparable pose and the same video settings on
the same device, so these numbers attribute one cause of the phone gap. They are not a parity
claim. The remaining gap is not submission, and the next candidate, per-fragment terrain
shading, is not yet measured in isolation.

## Method

Samsung SM-G781B (Snapdragon 865, Adreno 650, Android 16), 1080×2400 at 120 Hz, 5.6 GB RAM,
proprietary Qualcomm Vulkan driver, driver build 05/11/25. Render distance 5, FOV 93, render
resolution 100, MSAA off, fancy skies off, smooth lighting off, transparent leaves off, view
bobbing off, one saved local world with no other entities, fixed player pose at night. The GPU
ran at its 587 MHz maximum with 99% busy throughout; its frame time is the cost being compared.

Frames come from SurfaceFlinger's per-layer counter for the client activity (12 s windows,
reported as mean, p10, p50 and p90), and engine GPU frame time comes from the client's own
`CINNABAR_SLOW_FRAME` window stages. Pass sums from the F3 overlay are not used: those samples
overlap and include reversed timestamps, as recorded in [F3 overlay CPU cost](f3-overlay-cost.md).

Both paths ran in one process, alternating units and per-section draws with a measurement-only
toggle that a private build read from a marker file under the app's data directory. Each phase
settled 22 s, and the whole sequence ran in one session, so thermal drift and world streaming
state are shared. The toggle and its environment variable were not committed.

A run counts only when the client's own log reports terrain draws for that run. Earlier
restart-based pairs needed `adb install -r` per arm, and two arms never left the loading screen
(the client was still above 113 fps on its menu) and are discarded.

## Interleaved comparison at `08446aa3ae`

Six phases in one session, in this order:

| Phase | Path | Mean fps | p10 | p50 | p90 | Median GPU ms |
| ---: | --- | ---: | ---: | ---: | ---: | ---: |
| 1 | units | 27.9 | 26.7 | 27.3 | 29.5 | 36.6 |
| 2 | direct | 20.9 | 18.7 | 21.2 | 23.9 | 44.9 |
| 3 | units | 24.2 | 21.5 | 24.1 | 27.1 | 44.5 |
| 4 | direct | 22.3 | 21.1 | 21.6 | 23.9 | 45.1 |
| 5 | units | 25.8 | 24.1 | 26.0 | 27.1 | 37.7 |
| 6 | direct | 21.6 | 19.1 | 21.5 | 23.9 | 45.4 |

Units averaged 26.0 fps (38.3 ms median GPU) against 21.6 fps (45.4 ms) for per-section draws.
Every units phase beat both following and preceding direct phases; the smallest pair gap is
phase 3 against phase 4, 24.2 against 22.3 fps.

The measurement build's absolute numbers only serve this paired comparison. A clean production
build of the same commit, with no toggle, measured 23.9 fps at the same pose, within the drift
range below.

## Restart-based comparison before #3070

At `0882bd81a0`, three back-to-back process pairs on the same pose and settings:

| Pair | Units fps | Direct fps |
| --- | ---: | ---: |
| 1 | 17.1 | 20.9 |
| 2 | 20.0 | 22.3 |
| 3 | 23.2 | 24.0 |

Absolute values move by more than the effect between sessions, so only the direction within a
pair is used: per-section draws were 3–22% faster than batching without the ordering fix, and,
with it, 8–25% slower than the units phase before each of them (the interleaved table above).
The cross-session magnitude is not a comparable measure of the ordering change; the 20% figure
comes from the interleaved run.

## GPU pass breakdown

One session at a close-up terrain pose, using the client's opt-in pass profiler
(`CINNABAR_GPU_CATEGORIES=1`, injected through `wrap.<package>` without rebuilding). The profiler
splits the opaque pass into category passes with extra loads and stores, so these numbers
attribute cost inside the frame and are not themselves a frame-time claim. Median of six
report windows:

| Pass | 100% render | 50% render |
| --- | ---: | ---: |
| GPU frame | 25.0 ms | 17.8 ms |
| opaque, total | 16.9 ms | 6.9 ms |
| solid terrain | 15.1 ms | 5.9 ms |
| terrain models | 1.1 ms | 0.6 ms |
| transparent terrain | 3.4 ms | 3.3 ms |
| sky | 2.2 ms | 0.6 ms |
| blit | 1.9 ms | 1.1 ms |
| hand | 0.3 ms | 0.2 ms |

Solid terrain is three fifths of the GPU frame. It scales with pixels, from 15.1 ms at full
resolution to 5.9 ms at half width and height; a linear fit through those two points attributes
about 13 ms of the pass to pixel-dependent work, whether shading or overdraw, and about 2 ms to
geometry and submission. That fit assumes the pass keeps scaling to zero pixels and ignores the
profiler's extra loads and stores, so it is an estimate, not a bound. Transparent terrain barely
moves, 3.4 against 3.3 ms, which leaves submitted geometry as the candidate for its cost; fixed
profiler overhead could contribute too, and no measurement separates them yet. The same pose
measured 37.0 fps at full resolution and 52.7 fps at half.

Applying that estimate, removing every pixel-dependent millisecond of the solid pass would leave
around 12 ms of frame, still above vanilla's whole 8.8 ms, because the transparent, sky, blit and
remaining passes would stay. That arithmetic is provisional for the same reasons. Vanilla's
`options.txt` carries `target_resolution:1`; this document does not establish what that key
controls or vanilla's render-target size, so it does not show whether both clients render the
same number of pixels.

## Remaining gap

The batched path now issues three unit draws, and both paths draw solid terrain front-to-back in
the early depth pass. The per-section path still submits about 400 draws and keeps cutout
sections in visibility order; both differences remain after the batched path started ordering
sections nearest first. Submission does not explain the remaining gap: the pass breakdown puts
three fifths of the GPU frame in the solid terrain pass, and most of that is pixel-dependent.

The unverified candidate is per-fragment work in the terrain shader that both paths share: a
texture-array sample, a per-fragment biome tint with transfer-function `pow` calls, the native
lightmap fetch and distance fog. The pass breakdown above shows the pixel-dependent part of the
solid pass, but it cannot separate shading from overdraw, and no measurement yet isolates one
shader feature. Vanilla's simpler terrain shader draws the same scene in 8.8 ms per frame at
113.1 fps, which is consistent with that attribution but is not evidence for it. Isolating the
cost of that fragment work is missing acceptance evidence, so this attribution stays provisional.

Related: lower render resolution helps but does not close the gap. Half width and height raised
the same pose from 37.0 to 52.7 fps, still well under vanilla's 113.1, because the fixed passes
and the remaining pixel work keep the frame above 17 ms. At 10% render scale the FSR1 EASU/RCAS
upscale to 2400×1080 dominated instead, 58.1 ms of GPU time against 35.6 ms at full resolution in
one session, so the comparisons in this document all ran at full render resolution.

## Measurement hazards

- Orphaned world servers (`libbds_loader.so`) survive `am force-stop` and hold about 1.5 GB and
  CPU time. Left running, the next session swaps (5.5 of 5.6 GB) and the activity is killed with
  `Exit reason: 3 … device ran low on memory`. Kill them before each run.
- The GPU stays at its 587 MHz maximum and 99% busy, at 49–56 °C battery temperature. Identical
  work measured between 17 and 29 fps across hours, so only interleaved or paired comparisons
  are trustworthy.
- `wrap.<package>` is the only way to inject environment variables into the installed app
  without rebuilding: `setprop wrap.app.cinnabar.client 'env NAME=value'`, cleared with an empty
  value, and read by the app only at process start.
