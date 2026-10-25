# Temporal upscaling

**Settings > Video > Upscaler** chooses how the world is brought to output resolution:

- **FSR 1** (default) upscales each frame on its own with EASU, optionally sharpened by RCAS.
- **DLSS** (NVIDIA RTX, Vulkan graphics API) reconstructs detail across frames. At 100%
  Render Resolution it is DLAA, temporal anti-aliasing at native size. The quality mode follows
  Render Resolution: 100% is DLAA, and smaller sizes use the nearest of Quality (67%),
  Balanced (58%), Performance (50%) and Ultra Performance (33%). A size outside that mode's
  range keeps FSR 1 and logs why.

While a temporal upscaler is active, the world camera:

- jitters its projection by a sub-pixel Halton(2,3) offset each frame, with a negative texture
  LOD bias for the reduced resolution;
- renders single-sampled without SMAA;
- writes render-resolution motion vectors (`render::temporal`). Static geometry and the sky
  reproject exactly through the camera. Hands, drawn with their own depth, get zero motion.
  Camera cuts, perspective changes, teleports and long frame gaps reset the upscaler's
  history.

The upscaler runs after tonemapping and before screen UI. Menus, the HUD and the crosshair stay
at native resolution, as with FSR 1. Shader-pack cameras never use a temporal upscaler; they
keep FSR 1.

Not yet covered: motion vectors for animated actors, dropped items and block entities, which
reproject with the camera only; a reactive mask for particles, clouds and weather, which don't
write depth; nametags and world-space UI, which are drawn into the jittered world before
upscaling; and hand silhouettes, which stay stair-stepped below 100% Render Resolution because
hands are absent from the depth the upscaler sees. FSR 3 is planned as a second temporal
upscaler.

## DLSS Super Resolution

DLSS runs through NVIDIA NGX on the Vulkan renderer only, behind the opt-in `dlss` cargo
feature of `cinnabar` (which enables `render/dlss`). Default builds and CI don't need the DLSS
SDK.

With the feature, renderer startup creates the Vulkan instance and device with the extensions
NGX requests, keeping Cinnabar's own device features and limits. Any DLSS failure falls back to
the usual instance or device, so DLSS never stops a renderer from starting. The result is the
`render_scene::temporal::TemporalUpscalerSupport` resource in the main and render worlds;
without the feature, or on DX12, Metal or GL, its `dlss` field is `false`. The startup log shows
why DLSS is unavailable.

### Building

You need:

- the [DLSS SDK v310.9.1](https://github.com/NVIDIA/DLSS/tree/v310.9.1), the version
  `dlss_wgpu` 6.0.0 targets. Read its licence first.
- Vulkan headers laid out as a Vulkan SDK (`Include/vulkan/*` on Windows).
- clang for bindgen.

Set absolute paths, then build:

```sh
DLSS_SDK=/path/to/DLSS VULKAN_SDK=/path/to/VulkanSDK LIBCLANG_PATH="/path/to/LLVM/bin" \
  cargo build -p cinnabar --features dlss
```

`Cargo.lock` resolves `bindgen` against `syn` 2, the version its `prettyplease` uses; a
resolution that picks `syn` 3 for bindgen fails to compile.

### Running

NGX loads `nvngx_dlss.dll` at runtime. It searches `$DLSS_SDK/lib/Windows_x86_64/rel` when
`DLSS_SDK` is set and the executable's directory otherwise. For a dev build, copy it next to the
executable:

```sh
cp "$DLSS_SDK/lib/Windows_x86_64/rel/nvngx_dlss.dll" target/debug/
```

Then choose Vulkan as the Graphics API, or set `WGPU_BACKEND=vulkan`. Never commit the DLL.
Once in a world with Upscaler set to DLSS, the log shows the mode and sizes in use, such as
`DLSS Quality reconstructs [1280, 720] to [1920, 1080]`.

Packaging is not done yet: shipped builds don't include the DLL or the NVIDIA licence notices
from section 9.5 of the DLSS programming guide.
