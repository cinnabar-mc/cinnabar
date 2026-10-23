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

- Unknown block `render_method` values become cutout; the target uses opaque.
- Block material instances are capped at 64; the target reads every entry.
- `packed_bools` bits 1 to 4 (isotropic, texture variations, alpha-masked tint and an internal
  vertex flag) are discarded.
- Server texture sets contribute only their colour layer.
- Entity materials keep three states, three defines, two depth functions and three blend pairs.
  Variants, stencil, samplers, vertex fields, depth bias, MSAA and primitive mode are dropped.
- Multitexture and colour-mask behavior is selected by pinned texture hashes, not by defines.

These questions remain unresolved:

- the `*_to_opaque` distance threshold;
- the visual meaning of packed bit 4;
- the minimum format version for texture variations;
- the render-target key literal;
- draw-time variant selection;
- the separate `metalness`/`emissive`/`roughness` texture-set form.
