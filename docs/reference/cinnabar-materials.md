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
- Bindings apply in document order and the last match wins. `entity` matches an entity or
  attachable identifier exactly; `render_controller` and `bone` are `*` patterns, and omitting one
  matches everything. A binding without `bone` replaces the controller's whole material.
- `block` bindings take an exact block identifier, an exact-value subset of its `states` and a
  `material_instance` pattern.

Status: entity bindings render. Block bindings, textures and parameters are validated but not yet
drawn; no host program reads them.
