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

- Texture variations, alpha-masked tint and packed bit 4 are decoded and retained but not
  rendered. The alpha-masked tint shader formula and the bit 4 constant are unknown.
- AO exponents above 2.55 or finer than hundredths keep the default exponent.
- `*_to_opaque` methods render as their near-distance method everywhere; the distance threshold is
  unknown.
- Server texture sets contribute only their colour layer.
- Entity materials resolve completely, with version rules, `+name` modification, same-file
  parents and variants, into the carrier's material table. Actor pipelines draw each layer with
  its definition's culling, depth function, depth writes, blend factors, colour and alpha writes
  and alpha to coverage. Stencil state is carried but not rendered because scene depth has no
  stencil aspect; depth bias is carried but not rendered. Samplers, vertex fields, MSAA support,
  primitive mode and shader defines other than alpha to coverage are not applied.
- Colour masking and three-sampler draws follow `USE_COLOR_MASK` and `USE_MULTITEXTURE`. The
  vanilla pack does not define the native `sheep`, `wolf` and `llama` materials, so the client's
  built-in bases declare them; packs cannot derive from `entity_change_color` or
  `entity_multitexture`, which the vanilla pack never names. `GLINT`,
  `MULTIPLICATIVE_TINT` and `MASKED_MULTITEXTURE` texture requirements are not checked.

These questions remain unresolved:

- the `*_to_opaque` distance threshold;
- the visual meaning of packed bit 4;
- the minimum format version for texture variations;
- the render-target key literal;
- draw-time variant selection;
- the client's depth bias scale;
- the separate `metalness`/`emissive`/`roughness` texture-set form.
