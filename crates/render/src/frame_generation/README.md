# Frame generation

Frame Generation is an optional Video setting, off by default. It inserts one
interpolated SDR world image between real images. Buffering and extra GPU work add
input and presentation latency. Hardware cost, visible pacing and added latency
still need the qualification below; software Vulkan fixtures cannot establish them.
The GPU algorithm retains the Horizon/FidelityFX MIT notices in
`THIRD_PARTY_NOTICES.md`.

## Runtime contract

The native client enables `render/frame-generation`. Preparation runs on finite
workers. Ordinary real rendering continues while pipelines and storage are
pending. Failed configurations stay bypassed until the configuration changes or
the setting is disabled and enabled again. Resizes, device changes, MSAA and Render
Resolution changes prepare new storage before use.

Only a complete, opaque primary-window SDR view can add presentations. Other
cameras, partial/shared outputs, HDR, Enhanced rendering, shader packs, motion
blur, projected UI and screen effects bypass generation. Screenshots and developer
recordings use ordinary real frames. The browser has no extra surface presenter.

The render thread retains the real surface image, generates a world midpoint,
replays current UI, and presents the midpoint followed by the owned real image.
This extra schedule contains UI encoding only: it never runs application updates,
input sampling, extraction, picks, interaction or simulation. Backend surface
presentation uses wgpu on Vulkan, DX12 and Metal.

Admission requires at least the real-frame rate in
`render_api::frame_generation::FRAME_GENERATION_MIN_FPS`. Synchronized output also
needs a known refresh rate with capacity for two presentations per real frame.
Cuts, player/server camera reanchors, teleports, dimension transitions and bypassed
intervals invalidate history. A late or incomplete candidate presents the real
frame and resets pacing. GPU scene-change detection can additionally replace an
interpolation with current world colour.

The pacer uses a monotonic edge clock and absolute waits. After warming up the
buffered timeline, the generated submission is scheduled halfway between the
preceding and following real submissions. Submission deadlines do not prove
scanout times: the driver, vsync, compositor and display can change when photons
appear. Those require hardware measurement.

| Content | Composition and limits |
| --- | --- |
| Menus, HUD and chat | Replay the current native-resolution UI publication after world interpolation, including alpha and inversion ordering. Incomplete composition bypasses the extra presentation. |
| First-person hand | Retain current colour and its own depth/coverage separately. Composite it after names on both images; never include it in world history. |
| Nametags | Draw current names against original world depth, preserve encoded per-sample blending, and composite before the hand. They keep the world's spatial resolution and are never temporally warped. |
| Particles and translucency | Remain in world colour. Optical flow tracks image motion; opaque depth cannot describe their independent depth. In-world artifact qualification remains open. |
| Shader packs, Enhanced rendering, HDR and motion blur | Bypass. |
| Screen effects and projected UI | Bypass while present. |
| Render Resolution | Upscale clean world colour to native size before interpolation. Resolve depth at world resolution, including 10%. Native screen UI stays independent. |
| MSAA and SMAA | Resolve reverse-Z world depth and hand coverage separately. Preserve per-sample name blending. SMAA processes world colour before native composition. |

`FrameGenerator::prepare` owns pipelines and persistent storage. `generate`
submits optical flow, camera/depth reconstruction, interpolation and hole filling
in queue order. Its input is complete native-sized `Rgba8Unorm` encoded SDR RGB,
without hand, names or screen UI. Depth is reversed `Depth32Float` at world size;
finite and infinite-far projections are supported. Output is `Rgba16Float` encoded
SDR RGB. Its alpha is metadata, never UI coverage. Invalid input or explicit resets
return no usable midpoint. Generated pixels never feed gameplay.

## Hardware qualification recipe

Use one hardware GPU and a display at 120 Hz or above. Record OS, GPU, driver,
backend, display refresh, compositor/VRR state, resolution and build SHA. Disable
HDR and VRR for the initial comparison. Use a release build and the same offline
world, camera route and settings for each run. Keep Render Resolution at 100%,
motion blur off, no shader pack and a real-frame cap of 60 FPS. Confirm that the
base rate stays above the admission threshold. Repeat at 1920x1080 and 2560x1440,
with Frame Generation off/on and vsync off/on. Use Vulkan and DX12 on Windows and
Metal on macOS; a successful run on one backend does not qualify the others.

1. Warm each configuration for 120 real frames. Use RenderDoc/Vulkan GPU timing,
   PIX/DX12 timing or Xcode/Metal GPU profiling to capture at least 600 real
   frames. Verify that extra surface presentations actually occur and that no
   candidate was bypassed. Measure GPU timestamps, excluding CPU waits. Report
   interpolation compute alone and total added GPU work, including world copies,
   foreground/UI replay and the real-image surface blit. Divide the extra work
   by the number of generated presentations; report median, p95 and p99 per
   generated frame. Keep the same count of real frames in the off/on comparison.
2. Measure input-to-photon externally; screenshots and developer recording bypass
   generation. Film a mouse-button electrical LED and the first changed hand
   animation pixels together at 1000 FPS or use equivalent input/photodiode
   instrumentation. Collect at least 100 randomly phased clicks in each off/on
   configuration, with the same viewport region and input device. Report median,
   p95, measurement resolution and the on-minus-off latency distribution. Also
   inspect real/generated scanout intervals; CPU presentation logs alone are
   insufficient.
3. Repeat a slow pan, translation, abrupt turn, teleport/dimension transition,
   setting toggle, resize, MSAA switch, Render Resolution switch and screenshot.
   Check HUD/chat/crosshair edges, the hand, names, particles and translucent
   surfaces. Confirm no old-world image crosses a cut, no UI warps and no first-use
   compile blocks rendering. Record every bypass and attributable hitch.

Synthetic headless tests establish composition and scheduling contracts only.
Hardware GPU cost, actual halfway scanout, latency and in-world artifact acceptance
remain open until these runs pass.
