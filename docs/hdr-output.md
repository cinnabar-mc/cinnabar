# HDR output

Turn on **Settings > Video > HDR** to present through your display's HDR mode. HDR also
needs HDR turned on in your system display settings; while the display reports SDR the
game stays SDR and switches over when the system enables HDR. The swapchain uses scRGB
where the platform offers it and HDR10 (PQ) otherwise.

- **HDR Peak Brightness** (400–4000 nits, default 1000) is the brightest level
  highlights reach. Match it to your display's peak.
- **HDR Paper White** (80–500 nits, default 200) is the brightness of white menus,
  the HUD and SDR content.
- **HDR Highlight Boost** (0–100%, default 50%) applies to Vanilla and shader-pack
  scenes, which are tonemapped for SDR. Colour below a knee is unchanged; brighter
  colour expands toward a white level partway between paper white and the peak.
  0% shows these scenes exactly as SDR at paper white.

Enhanced renders a float scene and tonemaps it with the SDR curve up to a knee, then
rolls highlights off toward the peak instead of compressing them into white.

Menus, the HUD and the crosshair always draw at paper white, at native output
resolution, over the expanded or tonemapped world. Translucent UI blends over HDR
highlights in sRGB values as it does in SDR. Screenshots, recordings and world
thumbnails taken while HDR is on are converted to SDR at paper white, with highlights
clipped.

HDR adds a full-resolution copy of the world each frame when Render Resolution is
100%. The game does not send HDR mastering metadata; displays use their own defaults.
