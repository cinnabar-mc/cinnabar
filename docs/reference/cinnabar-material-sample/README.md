# Cinnabar material sample pack

A minimal resource pack showing [Cinnabar material bindings](../cinnabar-materials.md): the pig's
head bone draws a glow mask, and the `sample:lamp` custom block draws one while its `sample:lit`
state is true.

The pack ships no images. Add your own before loading it:

- `textures/sample/pig_glow.png`, the same size as the pig texture. Its alpha selects which texels
  glow.
- `textures/sample/lamp_glow.png`, a terrain-sized tile, which `textures/terrain_texture.json` names
  `sample_lamp_glow`.

The lamp binding needs a behavior pack that defines `sample:lamp` with a boolean `sample:lit` state.
Clients without Cinnabar ignore `cinnabar/material-bindings.json`.
