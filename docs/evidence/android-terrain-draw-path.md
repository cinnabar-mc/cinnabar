# Android terrain draw path on Adreno 650

On a Galaxy S20 FE 5G, the batched terrain unit path draws opaque terrain 18% faster than the
per-section fallback after the ordering fix in [#3070](https://github.com/cinnabar-mc/cinnabar/pull/3070)
(`08446aa3ae`): 26.0 against 21.6 frames per second, and 38.3 against 45.4 ms median GPU frame
time, with both paths drawing 296 sections. Before #3070 the same comparison, run as separate
processes, put the per-section path ahead. Disabling batching on proprietary Adreno Vulkan is
therefore a regression after #3070, not a workaround.

Vanilla Bedrock 1.26.60 measures 113.1 fps at a comparable pose and the same video settings on
the same device, so these numbers attribute one cause of the phone gap. They are not a parity
claim, and the remaining gap is fragment shading rather than submission.

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

## Restart-based comparison before #3070

At `0882bd81a0`, three back-to-back process pairs on the same pose and settings:

| Pair | Units fps | Direct fps |
| --- | ---: | ---: |
| 1 | 17.1 | 20.9 |
| 2 | 20.0 | 22.3 |
| 3 | 23.2 | 24.0 |

Absolute values move by more than the effect between sessions, so only the direction within a
pair is used: per-section draws were 3–22% faster than batching without the ordering fix, and
are 9–33% slower with it (the interleaved table above).

## Remaining gap

The batched path now issues three unit draws, and both paths draw solid terrain front-to-back in
the early depth pass. The per-section path still submits about 400 draws and keeps cutout
sections in visibility order; both differences remain after the batched path started ordering
sections nearest first. Both paths share the same terrain shader and full render resolution, so
the bulk of the remaining 38 ms is per-fragment work: a texture-array sample, a per-fragment
biome tint with transfer-function `pow` calls, the native lightmap fetch and distance fog. That
is also why vanilla's simpler terrain shader still draws the same scene in 8.8 ms. Neither draw
path closes that gap.

Related: reduced render resolution is not a workaround on this device. At 10% render scale the
FSR1 EASU/RCAS upscale to 2400×1080 dominated the frame, and the F3 overlay read 58.1 ms of GPU
time against 35.6 ms at full resolution in one session. The comparisons above all ran at full
render resolution.

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
