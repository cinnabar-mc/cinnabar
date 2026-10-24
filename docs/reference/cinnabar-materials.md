# Cinnabar material bindings

A resource pack may add `cinnabar/material-bindings.json` to give Cinnabar clients materials that
Bedrock files cannot attach. Vanilla clients ignore the file. Only the highest pack's copy is read;
an invalid copy is ignored whole (the client logs why) and Bedrock materials stay unchanged.

```json
{
  "format_version": "1.0.0",
  "materials": {
    "example:glow": {
      "base": "entity_alphatest",
      "material": { "+states": ["Blending"], "+defines": ["USE_EMISSIVE"], "blendSrc": "One", "blendDst": "One" },
      "textures": { "glow_mask": "textures/example/glow_mask" },
      "parameters": { "strength": { "scalar": 2.0 } }
    }
  },
  "bindings": [
    { "entity": "example:moth", "bone": "wing*", "material": "example:glow" },
    { "block": "example:lamp", "states": { "example:lit": true }, "material_instance": "*", "material": "example:glow" }
  ]
}
```

- `material` takes the fields of a Bedrock `.material` declaration, read with version 1 rules
  over `base` (a Bedrock material the pack or client defines) or the constructor defaults. The same
  resolver and lowering as `.material` files apply, so every state, define and blend factor the
  client renders for Bedrock materials renders here. On entities a `fragmentShader` whose stem
  is not `entity` or `pattern` draws as plain `Actor`, as on Bedrock; a material with neither
  `base` nor `fragmentShader` draws with the entity program.
- `textures` maps names to pack-relative paths inside the pack. `parameters` are `scalar`,
  `vector` (two to four numbers), `color` (RGBA) or `boolean`.
- Two texture names draw masks on the base texture's coordinates, scaled by the `strength`
  scalar (default 1):
  - `glow_mask` adds `rgb × alpha × strength` after lighting, so it shows in the dark;
  - `emissive_mask` draws texels unlit, weighted by `alpha × strength`.
  On entities the value is a texture path and the mask must be the same size as the texture it
  masks. On blocks the value is a `terrain_texture.json` key, as material instances name their
  textures; weighted-variation faces and sorted blended model faces draw without masks.
- Bindings apply in document order and the last match wins. `entity` matches an entity or
  attachable identifier exactly; `render_controller` and `bone` are `*` patterns, and omitting one
  matches everything. A binding without `bone` replaces the controller's whole material.
- `block` bindings take an exact block identifier, an exact-value subset of its `states` and a
  `material_instance` pattern. Vanilla blocks bind as a whole, with no `material_instance` or
  `*`: untinted full cubes through their faces, and model blocks (stairs, panes, fences, doors,
  plants and their pot contents included) through every quad they can draw; tinted full cubes,
  nether portals, lily pads and models drawing art outside `terrain_texture.json` keep their base
  materials. They replace the matching instances' render method with the closest one the
  material lowers to: alpha blending, alpha test, opaque, one- or two-sided, and bound model quads
  take that sidedness. Block passes have no other blend factors, depth functions or write masks; a
  binding that needs them draws approximately and counts as an approximated material in the
  session's block report.

The [JSON Schema](cinnabar-material-bindings-1.0.0.schema.json) (Draft 2020-12) describes format
1.0.0. A test keeps it in step with the client's parser. The client also rejects a binding that names
an undeclared material and parameter numbers that are not finite 32-bit floats, and it counts name
and pattern limits in UTF-8 bytes; the schema cannot express these.

The [sample pack](cinnabar-material-sample/README.md) binds a glow mask to an entity bone and to a
custom block state; it ships no images.

Status: entity and block bindings render with their masks. Other textures and parameters are
validated but not drawn.
