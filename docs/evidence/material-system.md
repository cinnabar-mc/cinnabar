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
- Server block texture sets keep their normal or height and MER or MERS layers in sparse Enhanced
  pages keyed by the overlay texture their colour occupies, charged to the terrain byte budget.
  Server-authored slots win over local Enhanced maps. Only builds that can draw Enhanced decode
  them, and the Enhanced renderer is disabled, so non-Enhanced draws use colour only, as Bedrock
  does outside Vibrant Visuals. A set that does not decode or fit keeps its colour and is counted. Their images join the
  block reload fingerprint, so an unchanged reload neither decodes nor combines them again, and a
  changed pack releases the previous maps.
- Entity texture references resolve through a same-pack texture set to its colour layer, which is
  all an entity draws outside Vibrant Visuals. Entity MER/MERS and normal layers are not carried
  yet; a future Enhanced actor path would need them.
- Entity material files load through the entity material list, `materials/fancy.json` (the
  client draws with fancy graphics): every pack's copy is read, each entry's defines seed the
  root of the file it names and `tag` is ignored. The vanilla lists are not shipped to the
  client, so `materials/*.material` files no list names still load after the listed ones;
  Bedrock loads only listed files. The order in which Bedrock processes the packs' list copies is
  not settled; the client reads the highest pack's copy first.
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
- A material's `fragmentShader` stem selects its actor material type. A program that is not
  `entity`, `pattern` or an elided third stem draws as plain `Actor`, so the client
  drops its three-sampler draw; the absent variant flags' defaults and the per-type material
  tables are unknown, so `pattern` draws as `entity`.
- Actor draws bind their extra textures and colours as Bedrock wires its defines: a second
  texture for `GLINT`, `MULTIPLICATIVE_TINT`, `USE_MULTITEXTURE` and `MASKED_MULTITEXTURE`, a
  third for `USE_MULTITEXTURE`, the overlay colour with `USE_OVERLAY` or `TINTED_ALPHA_TEST`, and
  `uv_anim` with `USE_UV_ANIM`. The shader formulas are unknown, so the glint (the
  client's actor glint), multiplicative tint, masked multitexture and coloured second texture
  draws are provisional; `MULTI_COLOR_TINT` and `COLOR_BASED` are carried but not drawn.
- Colour masking and three-sampler draws follow `USE_COLOR_MASK` and `USE_MULTITEXTURE`. The
  vanilla pack does not define the native `sheep`, `wolf` and `llama` materials, so the client's
  built-in bases declare them; packs cannot derive from `entity_change_color` or
  `entity_multitexture`, which the vanilla pack never names. A material
  whose `GLINT`, `MULTIPLICATIVE_TINT` or `MASKED_MULTITEXTURE` defines need more textures than its
  controller supplies is not drawn; whether the native check applies to every material is
  unresolved.

Each server pack's entity compilation counts, and logs once, the material behaviour the client
does not draw exactly: unknown state names (Bedrock asserts), define names without an actor flag
(ignored silently, as Bedrock does), sampler entries at index 8 or above (skipped, as Bedrock
does), materials that fail to resolve, programs without an actor type, stencil, polygon-offset or
depth-bias state, carried-but-undrawn actor defines, draws dropped for missing textures, unlisted
material files and partly read list entries.

Actor pipelines are keyed by the lowered raster contract, a finite set; block and block-entity
pipelines use a fixed key per view. An unchanged pack reload requests no pipelines, and a pack change
retires the previous session's actor pipelines from readiness. Bevy's pipeline cache cannot destroy
pipelines, so retired ones stay compiled and are reused if a later pack needs them again. An actor
material whose state the adapter's render targets cannot represent (blending into a format that does
not blend there, or a sample count the format does not support), or whose pipeline fails to compile,
draws with its view's unauthored pipeline; each such material is logged and counted once, and
readiness does not wait on it.

Texture variations have no client version gate, only the network bit. The
separate `metalness`/`emissive`/`roughness` texture-set form merges when all three layers are
present.

These questions remain unresolved:

- the `*_to_opaque` distance threshold;
- the visual meaning of packed bit 4;
- the render-target key literal;
- how a material's depth bias reaches an actor draw, and its scale;
- the backend addressing and filtering a material's sampler states select;
- the order in which packs' copies of the entity material list are processed, and which files the
  vanilla lists name;
- whether a flipbook texture resolves through a texture set and how its surface layers animate.
