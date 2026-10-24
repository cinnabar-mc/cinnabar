# Material system evidence

This is the behavior baseline for Bedrock protocol 2193 materials. The machine-readable
matrix is [material-behavior.tsv](material-behavior.tsv). It covers entity
`.material` files, render-controller bindings, block material instances and texture sets.

Evidence records observed native client behaviour. An older reference build is used only
to name behaviour the target does not show. The matrix records behaviour, never code.

Each row gives one of these target statuses:

- `matched`: the target client shows the behavior.
- `unresolved`: the target's behavior is not settled, for example because a key literal is elided or a
  threshold constant is not visible.
- `older-only`: only an older reference shows it. Such a row cannot close a parity gate.

The disposition describes the current client: `implemented`, `partial` or `unsupported`. Every
`implemented` row names its proving test as `path::function`, and the architecture test fails when
that test does not exist. Fixtures in `material-fixtures/` are original synthetic data, anchored by
RFC 6901 pointers. The network NBT fixture lives with the protocol crate.

`material-source-inventory.tsv` lists the pack and wire keys the client parses. The architecture
test requires every inventory token to appear in a matrix row's surface, wire or conditions column.

Confirmed gaps against the current client:

- Custom block texture variations draw like vanilla terrain variations. Alpha-masked tint uses
  the grass-side overlay composition (alpha weights the tint, the face stays opaque); the Bedrock
  shader formula is unknown, so this is provisional. Packed bit 4 is retained but not
  rendered; its constant is unknown.
- Block AO exponents reach the material as the exact authored f32, as Bedrock carries them; the
  shading consumer is unknown, so raising vertex AO to the exponent is provisional.
- `*_to_opaque` methods render as their near-distance method everywhere; the distance threshold is
  unknown.
- Server texture sets contribute only their colour layer.
- Entity materials resolve completely, with version rules, `+name` modification, same-file
  parents and variants, into the carrier's material table. Actor draws use a material's
  `skinning` variant, as Bedrock does, with its culling, depth function, depth writes, blend
  factors, colour and alpha writes and alpha to coverage. Stencil state is carried but not
  rendered because scene depth has no stencil aspect. Depth bias, MSAA support and sampler states
  are carried but not applied: no actor-path consumer of the first two was found, and the backend
  meaning of the sampler bits is unknown. Primitive mode does not affect actors on
  Bedrock either; their topology comes from the mesh.
- Entity material declarations, interned definitions and names have implementation bounds
  (16,384 declarations, 16,384 definitions, 256-byte names) that are not verified against the
  native client, which shows none. A material past them stays unresolved, is counted in the
  server pack's skip report and draws as a layer without a material.
- Colour masking and three-sampler draws follow `USE_COLOR_MASK` and `USE_MULTITEXTURE`. The
  vanilla pack does not define the native `sheep`, `wolf` and `llama` materials, so the client's
  built-in bases declare them; packs cannot derive from `entity_change_color` or
  `entity_multitexture`, which the vanilla pack never names. A material
  whose `GLINT`, `MULTIPLICATIVE_TINT` or `MASKED_MULTITEXTURE` defines need more textures than its
  controller supplies is not drawn; whether the native check applies to every material is
  unresolved.

These questions remain unresolved:

- the `*_to_opaque` distance threshold;
- the visual meaning of packed bit 4;
- the minimum format version for texture variations;
- the render-target key literal;
- how a material's depth bias reaches an actor draw, and its scale;
- the backend addressing and filtering a material's sampler states select;
- the separate `metalness`/`emissive`/`roughness` texture-set form.
