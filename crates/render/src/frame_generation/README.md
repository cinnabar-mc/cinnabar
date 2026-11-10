# Frame interpolation GPU backend

The `render/frame-generation` build feature exposes a GPU backend without enabling
frame generation in the client. It has no production caller, Video control or
surface presenter yet. This is an adapted Horizon/FidelityFX implementation; both
MIT notices are retained in `THIRD_PARTY_NOTICES.md`.

`FrameGenerator::prepare` compiles pipelines and allocates storage during loading
or on a worker. A resize or Render Resolution change needs a new prepared context.
Until that context is ready, present real frames. `generate` submits one pair's
optical flow, camera/depth reconstruction, disocclusion, interpolation and hole
filling in queue order. Its output never feeds simulation, picks or interaction.
The previous input is retained after each successful submission. Invalid inputs
invalidate history. Explicit resets return the current image and must never
schedule an extra presentation. Samples that cannot see the previous camera use
current colour and contribute no camera motion to reconstructed fields.

## Input and composition contract

Colour is a complete native-sized, single-sampled `Rgba8Unorm` image containing
sRGB-encoded world RGB. Do not pass an sRGB texture view: the kernels decode colour
where needed. Depth is finite reversed `Depth32Float`, single-sampled and sized to
the world render. The reprojection maps current unjittered clip coordinates to the
previous real camera. The output is native-sized `Rgba16Float` containing the same
encoded RGB, not linear RGB; its alpha is interpolation metadata, not UI coverage.

Screen UI must be composited separately onto every presented image. The backend
cannot separate UI that was already drawn into its colour input. These are the
requirements for the future runtime consumer:

| Content or setting | Required handling before enabling runtime generation |
| --- | --- |
| Menus, HUD, chat and screen overlays | Composite native pixels after interpolation on both real and generated images. Bypass if native composition is unavailable. |
| First-person hand and sharp nametags | Exclude from the input and composite separately; bypass until their independent colour/depth coverage is available. |
| Particles and translucency | Included in world colour; optical flow can track their image motion, but opaque depth cannot resolve their independent depth. Qualify artifacts before enabling them. |
| Shader packs and Enhanced rendering | Bypass until their colour encoding, depth and composition boundary meet this contract. |
| Motion blur | Bypass until ordering and image-history quality are qualified. |
| Render Resolution | Depth may differ from native colour size, including 10%; optical-flow fields use their own grid and stride. Upscale colour before this backend. Reprepare after either size changes. |
| MSAA and temporal AA | Resolve depth to one sample before generation. Supply actual projection jitter; otherwise bypass. |
| HDR | Bypass; this backend accepts SDR only. |

## Remaining runtime work

The opt-in Video control stays unexposed and off. A separate presenter must pace
one candidate halfway between real images without changing input or tick timing,
handle vsync and unsynchronized output on Vulkan, DX12 and Metal, and bypass low
base rates, missed deadlines, cuts, teleports and dimension changes. Call
`invalidate` for every bypassed interval so history never bridges a gap. CPU reset
metadata does not report GPU scene-change detection; the kernels can additionally
return the real image when their luminance histograms detect a cut.

GPU fixtures establish motion and reset behavior only. They do not establish UI
composition, latency, displayed-frame pacing or in-world visual acceptance. The
Video description must include measured added latency before runtime rollout.
