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
  client renders for Bedrock materials renders here.
- `textures` maps names to pack-relative paths inside the pack. `parameters` are `scalar`,
  `vector` (two to four numbers), `color` (RGBA) or `boolean`.
- On entities, two texture names draw masks on the base texture's coordinates, scaled by the
  `strength` scalar (default 1):
  - `glow_mask` adds `rgb × alpha × strength` after lighting, so it shows in the dark;
  - `emissive_mask` draws texels unlit, weighted by `alpha × strength`.
  A mask must be the same size as the texture it masks; otherwise it is not drawn.
- Bindings apply in document order and the last match wins. `entity` matches an entity or
  attachable identifier exactly; `render_controller` and `bone` are `*` patterns, and omitting one
  matches everything. A binding without `bone` replaces the controller's whole material.
- `block` bindings take an exact custom block identifier, an exact-value subset of its `states`
  and a `material_instance` pattern. They replace the matching instances' render method with the
  closest one the material lowers to: alpha blending, alpha test, opaque, one- or two-sided. Block
  passes have no other blend factors, depth functions or write masks; a binding that needs them
  draws approximately and counts as an approximated material in the session's block report.

Status: entity and custom-block bindings render, and entity bindings draw their masks. Other
textures and parameters, block masks and vanilla blocks are validated but not drawn.
