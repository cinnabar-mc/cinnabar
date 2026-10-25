# Server client parts

A client part is optional code offered by a server. Build a paired mod with
[Cosmic](modding/src/appendix/paired-mods.md); host server callbacks with the
[Experience runtime](experience-runtime.md). Personal client mods have
[separate user grants](modding/src/appendix/personal-mods.md).

Execution currently requires `CINNABAR_DEV_SERVER_EXPERIENCES=1`. Production
restricted launch is unavailable: consent alone does not enable production
execution. Developer helpers are separate processes, without an OS sandbox.

## Delivery and consent

The server advertises `cinnabar/extension-offer.json` in an optional resource pack.
The marker carries an Ed25519 server key and signed offer. The offer binds the
join destination, deployment revision, expiry, publisher keys, immutable bundle
digests, HTTPS origins, permission/budget scope and ordinary-client fallback.
Servers must retain ordinary gameplay when client parts are declined or fail.
The client currently cannot independently check the pack's optional provenance.

Consent choices are Allow once, Always, Never and Not now. Always pins the exact
destination, server key, scope and publishers; broader authority prompts again.
Never suppresses that destination, including changed keys. The prompt holds join
readiness, and approval requires its disclosure to have been drawn and scrolled
to the end. F9 revokes the offered/running extension. Trusted status and consent
cannot be replaced by resource packs.

An accepted resource pack grants no code authority. The client sends Hello only
after consent, verifies the signed live Accept, then checks/downloads bundles and
initializes helpers before Ready. Servers must wait for Ready before sending
application records. Timeout, denial, expiry, revocation or fatal helper failure
ends extension execution while ordinary gameplay continues.

Trust settings are in `server-experiences.json` under user data. Invalid settings
fail closed. Immutable bundle bytes may stay cached after leaving; grants, guest
state and pending output do not. Content updates take effect on rejoin.

## Permissions and budgets

Permissions must be in the signed manifest, offer, client capabilities and actual
grant. Requesting a permission does not supply an unimplemented adapter.

| Permission | Client contract |
| --- | --- |
| `ui` | Owned text labels; empty text removes one |
| `modal_ui` | Signed JSON-UI modal templates, values and collections |
| `input` | Declared focused-modal actions, text and modifier changes |
| `messaging` | Signed typed channels; client sends only `to_server` |
| `scene` | Declarative objects sharing the bundle's drawing snapshot and budgets |
| `media` | Playback of indexed signed descriptors |
| `items` | Read-only session item catalog |
| `world_draw` | Shared retained drawing, immediate primitives and local visual time/weather overrides |
| `entity_observe`, `entity_highlight` | Current native actor references and masks |
| `render_material`, `render_post`, `render_through_walls` | Bounded materials, post passes and through-wall drawing |
| `render_world_material` | Guest `shade` materials bound to the bundle's own entities and blocks (`world-material`, server-experience 1.6.0); developer client only |
| `render_world_material_override` | With `render_world_material`, bind any entity or block identifier, vanilla included (API 3); the developer client offers it only with `CINNABAR_DEV_WORLD_MATERIAL_OVERRIDE=1` |
| `render_world_material_vertex`, `render_world_material_scene`, `render_world_material_passes` | With `render_world_material`: vertex displacement, reading the view's opaque scene, and outlines, bloom, particles and compute (`world-material-effects`, server-experience 1.7.0, API 4); see [World material effects](#world-material-effects) |

A bundle asking for `render_world_material` signs a manifest `namespace` (API 2 and later): the
identifier namespace whose entities and blocks its world materials may bind. It holds
lowercase ASCII letters, digits, `_`, `-` and `.`, at most 64 bytes, and is never
`minecraft`. API 1 bundles are refused.

Client parts share the personal-mod drawing, render, environment and signed-asset
imports. Terrain, packet interception and local settings remain personal-mod APIs.
Client-part grants still come from the signed offer and consent.

| Limit | Authority |
| --- | --- |
| Callback/session fuel and cadence | `CALLBACK_FUEL`, `SESSION_FUEL`, `CALLBACK_INTERVAL_MS` in [runtime.rs](../crates/server-experience/src/runtime.rs) |
| Guest/session memory, output, bundles | `MAX_GUEST_MEMORY`, `MAX_SESSION_MEMORY`, `MAX_HOST_OUTPUT`, `MAX_BUNDLES` in [policy.rs](../crates/server-experience/src/policy.rs) |
| Declarations and signed manifest | `MAX_ACTIONS`, `MAX_CHANNELS`, `MAX_FILES`, `MAX_MANIFEST_BYTES` in policy |
| Typed messages, rates, queues, nesting | `MAX_MESSAGE_BYTES`, `MAX_MESSAGES_PER_SECOND`, `MAX_BYTES_PER_SECOND`, `MAX_BURST_BYTES`, `MAX_QUEUE_*`, `MAX_FIELD_DEPTH` in policy |
| Screens and text | `MAX_TEMPLATE*`, `MAX_TEXTURE*`, `MAX_COLLECTION*`, `MAX_ROW_FIELDS`, `MAX_UI_*`, `MAX_EDIT_TEXT_BYTES` in policy |
| Media | [media.rs](../crates/server-experience/src/media.rs) |

Callbacks stage output; successful callbacks publish whole transactions owned by
session, bundle, generation and world epoch. Traps discard that callback's output.
Per-session budgets prevent multiplying allowances by adding bundles. Initialization
also consumes the aggregate budget; Ready waits for all initializers.

A failed callback restarts its guest and counts a strike. Repeated failures stop
that part; other parts continue. Unusable helper output stops its producer; malformed
session framing ends the extension.
No WASI, arbitrary network, filesystem, account tokens or raw packets are imported.

## Callbacks and registry confirmation

Implement [`ClientPart`](../crates/experience-sdk/src/client.rs).

| Callback | Meaning |
| --- | --- |
| `init` | Initialize once after generated registry confirmation |
| `#[on_message]` | Decoded to-client message; generated dispatch validates the declared channel |
| `frame` | Optional presentation callback; coalesced while busy |
| `epoch` | World epoch changed; state remains live |
| `action`, `secondary_action` | Declared primary/secondary modal action, with collection row |
| `modal_resized` | First drawn size or GUI-size change |
| `text_changed` | Latest text per declared edit control |
| `scroll_changed` | Latest range per declared scroll view |
| `shift_changed` | Shift state while the modal owns focus |

Generated projects return `shared::REGISTRY`. Callbacks wait for its exact server
fingerprint, including after recovery. Matching confirmation initializes once;
mismatch stops the part and clears queued callbacks. While waiting, the host
retains latest state plus ordered messages/actions. Queue overflow stops that part.
After confirmation, latest epoch/modifiers/size/text/scroll state precedes queued
events. Old-epoch events are discarded.

## Drawing and signed assets

Use the [shared drawing contract](modding/src/appendix/drawing-helpers.md) for retained meshes,
materials, camera anchors, primitives and actor masks. References bind to the native
connection, dimension and actor incarnation. World changes clear old presentation;
stale references and dependent draws are dropped and counted without stopping the
part. Frames rotate across ready producers within the shared callback allowance.

Legacy objects and drawing publish in one immutable snapshot. Their draw and
triangle limits combine. Each bundle's GPU ledger covers retained and immediate
geometry, per-view instances, masks and media. Resident storage stays charged until
its last GPU use; buffer growth waits for old storage to retire when needed.

Low-level `[client].assets` declares signed drawing files beside `experience.toml`.
Generated declarations expose `Asset` constants; `assets.read_range` streams larger
files across callbacks. A file listed as a template, texture and asset is packaged
once. Source-driven projects retain their generated `shared::assets` declarations.
The [space client](../examples/experiences/space-client) shows a camera-anchored
planet with signed textures and a bounded procedural material.

## World materials

`render_world_material` lets a client part restyle its own entities and blocks with guest WGSL.
It is developer-only: production clients do not implement it, so it stays fail-closed until the
restricted-helper launch gate. The offer's consent text states it as one permission line:
"Change how this server's own mobs and blocks look (up to N MB GPU memory)", N being the
offer's GPU limit as the host enforces it.

`world-material.set-material(id, spec)` declares a material: WGSL defining
`fn shade(input: WorldMaterialInput) -> Surface`, at most four signed PNGs under `textures/`
(read with `texture(index, uv)`), at most sixteen finite numbers (read with
`param(input, index)`), a depth (`test`, or `always`, which also needs `render_through_walls`)
and a blend class (`opaque`, `cutout`, `premultiplied`, `additive`). `shade` receives the UV, world
position, Bedrock material colour (texture times tint), the light the host applies, the fog colour
and amount, the actor overlay (zero for blocks), the animation phase (Unix time modulo 4096 s
over 4096, so a server can time an effect's start on the same clock), the unit surface normal
and the unit direction from the surface to the camera; it returns colour, coverage and
emission. The host validates it with the drawing-material sandbox (no resources,
module state, overrides, entry points, loops, recursion or storage writes; bounded expressions and
texture reads per pixel), in the helper within four compiles per callback and again in the
client. `bind(id, target, material)` binds `entity(identifier)` or `block(identifier)`, which
must lie in the manifest's signed `namespace`; one target takes one binding, and a bound material
cannot be removed. No count limits a bundle: the host charges each declaration's source,
texture paths, parameters and identifiers to the bundle's share of the signed memory limit, and
each published material's textures and pipelines to its share of the GPU limit. The only fixed
bound is the tag width a target carries: a session can bind blocks to its first 63 materials and
entities to its first 255; a binding past that is reported and draws as Bedrock.

The host owns depth, blend factors, pass and visibility: a class selects the host's raster, and
the host applies Bedrock lighting and fog to the returned surface, adding emission after
lighting. A bound entity's body and render-controller layers draw with the material once its
pipelines are ready in every view, and with their Bedrock material, raster and pass until then;
equipment, capes, hats and players never do. A bound block's cube faces and model quads are tagged in
meshing and drawn in a world material pass: opaque and cutout classes in the opaque phase,
premultiplied and additive classes merged face by face into their section's ordinary
translucent order, so a bound face and the water or glass beside it draw back to front by the
face metric ordinary translucent terrain uses. A block bound to a cutout, premultiplied or
additive material hides no other block's faces, whatever its Bedrock look, as glass hides none:
the ground under a glass effect still draws. Touching faces of blocks bound to the same material
stay hidden, as glass hides glass. Until the material's pipelines are ready the block draws with
its Bedrock material.
Each material charges its shader source, parameters and decoded textures to the bundle's share of
the signed memory limit, and its textures and pipelines to its share of the GPU limit; replaced
materials stay charged because compiled pipelines cannot be destroyed. A material that fails
validation or native compilation is skipped, reported once in the log and the execution
indicator, and its targets keep their Bedrock materials. Stopping a client part, F9 or a
disconnect removes every material and binding in one publication without a reconnect.

With a shader pack, a bound block samples an atlas sprite of its own: each frame the guest's
`shade` renders the material over the sprite's square, every mip level, as the albedo with its
emission added and as LabPBR emission in the specular map, and the pack lights it as any block
sprite. The sprite sees a white base, an upward face seen from above and full light, so effects
that depend on world position, view direction or the block's tint are flattened; until the
first render, and after revocation, the block keeps its Bedrock sprite.

Not yet: Enhanced terrain draws bound blocks as Bedrock. Identifiers outside the signed
namespace, vanilla included, take `render_world_material_override`.

## World material effects

Three permissions extend `render_world_material`, each its own consent line, each denied by
default (the developer client offers one only when its `CINNABAR_DEV_WORLD_MATERIAL_*`
variable is `1`; production clients implement none of them), and each needs
`render_world_material` in the same manifest. `world-material-effects.set-material-with-effects`
declares a material with an `effects` record; the helper and the client both refuse an effect
whose permission is missing or whose numbers lie outside the host's ranges. Every effect draws
only the targets its material is bound to, and its pipelines, targets and cells are charged to
the bundle's share of the signed GPU limit, its declarations to the memory limit.

Any of the three can be withdrawn for the rest of the session without a reconnect (the
developer client's developer-control command `experience_permission` with `withdrawn`; there
is no in-game control yet): one publication drops exactly that permission's effects from every
material (a withdrawn
displacement draws undisplaced, a withdrawn scene reads no colour and infinitely far depth, a
withdrawn passes permission stops the bundle's outlines, bloom, particles and computes, and
cells read zero), and giving it back restores them. The material's declarations stay, so the
guest is not told. Pipelines compiled for a withdrawn form stay charged, as every replaced
material's do. Ending the runtime clears the withdrawals.

- **Vertex displacement** (`render_world_material_vertex`): the material's source also defines
  `fn displace(input: WorldVertexInput) -> vec3<f32>` (world position, normal, UV, phase), an
  offset in blocks that the host clamps on each axis to the declared `displacement-blocks`
  (at most 2). Bound entity and block draws run it per vertex (a bound block's faces stay one
  block wide so each block moves on its own); a bound entity's culling box grows by its
  material's reach and every terrain section holding bound blocks culls with bounds grown by
  the widest reach any material may have. It cannot move hit boxes, collision, selection,
  positions, nametags or anything gameplay reads. Shadows: the entity blob shadow stays under
  the entity's real position; shader packs draw bound blocks as their atlas sprite, undisplaced,
  in every pass including their shadow pass; Enhanced terrain draws bound blocks as Bedrock;
  actor motion vectors use the undisplaced pose.
- **Scene read** (`render_world_material_scene`): `scene_color(offset)` and
  `scene_depth(offset)` read the view's opaque colour and linear depth in blocks, already drawn
  this frame, `offset` pixels from the fragment and clamped to the screen, and
  `fragment_depth()` the fragment's own depth, for refraction, heat haze and soft intersections.
  Only premultiplied and additive classes may read it, since they draw after the opaque pass.
  While a published material reads the scene, each view copies its opaque colour (resolved
  when multisampled) and turns its depth into linear distance once the opaque pass and the
  entity shadows are drawn and before the translucent pass; a view whose copy is missing draws
  the material's targets with the base pipelines. Shader packs keep such a block on its Bedrock
  sprite.
  It cannot read anything not on that view's screen: no other view, no translucent or later
  pass, no UI, no previous frame, and nothing it reads ever reaches the guest, which has no GPU
  readback.
- **Extra passes and compute** (`render_world_material_passes`): after the translucent pass
  each view draws the targets of materials with an `outline` or a `bloom` once more into two
  effects targets, depth-tested against what is drawn, from the colour pass's own shading of
  each fragment: its emission times the bloom strength, and the colour of
  `fn outline(input: WorldMaterialInput) -> vec3<f32>`. Two fullscreen passes then blur the
  emission (strength at most 4, radius at most 16 pixels) and grow an outline at most 8 pixels
  wide around what can be seen of each target, never over it, and blend both over the scene.
  `particles` draws at most 4 096 camera-facing pixel squares per bound target within at most 4
  blocks of it (an entity's feet or a block's centre), placed and coloured by
  `fn particle(input: WorldParticleInput) -> WorldParticle`, as solid emissive squares
  depth-tested against the scene after the translucent pass; a view draws at most 65 536 a frame
  and the targets past that wait.
  `set-compute` declares at most 65 536 cells of four numbers the host owns for the bundle; each
  frame `fn step(input: WorldCellInput) -> vec4<f32>` returns each cell's next value from the
  previous frame's cells, and a material naming the compute in `cells` reads them with
  `cell(index)`. A bundle's computes update at most 262 144 cells a frame, and a frame runs at
most 1 048 576 cell updates across every bundle, before anything draws so every view reads the
same cells; a compute past that waits, and the next frame starts with one left out so every
compute steps in turn, and one whose pipeline fails to compile
is reported once while its cells read zero. Compute cannot write
  any other cell, read textures, host buffers or the scene, and nothing reads its cells back
  to the guest. The guest functions keep the sandbox's per-pixel, per-vertex or per-cell bound
  (no loops, bounded expressions and texture reads).

Ranges and charges are provisional: displacement at most 2 blocks, outlines 8 pixels, bloom
strength 4 and radius 16 pixels, 4 096 particles within 4 blocks, 65 536 cells per compute and
262 144 per bundle a frame; a bundle reading the scene pays 16 MiB once and one blooming or
outlining 16 MiB once, each extra pass and compute 256 KiB of pipelines, and each cell 32
bytes.

## Modal screens

Low-level bundles declare `templates`, `actions` and `textures` in the signed
manifest. `ui.open_screen` accepts only an indexed `ui/<name>.json`; `None`,
`close_screen` or Escape closes it. The template namespace must match the bundle
ID after replacing characters outside `[a-z0-9_]` with `_`. A bundle declares at
most `MAX_ACTIONS` (1024) actions; a press costs one lookup in that set.
Templates resolve against the shipped UI carrier, excluding server packs and
trusted consent catalogs. Images resolve from indexed `textures/`, then the
supported local art sources. URLs and paths outside `textures/` draw nothing.

A modal draws over gameplay only when no other screen is open, below trusted
status/toasts. While drawn, it owns pointer and keyboard input; F9 still revokes.
Bound values survive closing/switching screens.

| Binding/control | Contract |
| --- | --- |
| `ui.set_value` | Global `#name`: boolean, integer, finite number, text or bounded numeric array |
| `ui.set_collection` | Replace collection rows; each maps `#name` to a tagged value |
| `inventory_items`, `hotbar_items` | Host-owned read-only stack collections |
| `$pressed_button_name` | Declared action delivered on release |
| `button.menu_secondary_select` | Maps a secondary press to a declared action |
| `text_box_name` | Declared edit action; whole text coalesces per control |
| `ui.set_text` | Updates text within `max_length` without `text_changed` |
| `scroll_view_name` | Declared scroll action; reports offset, viewport and content in GUI units |
| `ui.modal_size` | Drawn root size in GUI units and GUI scale, otherwise none |

For full UI fixture code, see
[`terminal-client`](../examples/experiences/terminal-client).
The source-driven paired builder generates channels and assets; authoring modal
manifest entries uses the low-level bundle path below.

## Bundles and low-level tooling

A `.cxb` is an indexed ZIP of portable Wasm and assets with
`manifest.signed.json`. Every other file has an exact size and SHA-256 in the
publisher-signed manifest. Paths are portable lowercase relative paths; traversal,
links, duplicates, encryption and unsupported ZIP metadata are rejected. Only
stored/deflate entries are accepted. Native Wasmtime artifacts are refused.
Signatures establish the chosen publisher and integrity, not code safety.
The signed manifest is at most `MAX_MANIFEST_BYTES` (4 MiB) and its payload,
which the document hex-encodes, at most half of that: enough for `MAX_ACTIONS`
actions at the 96-byte identifier limit and `MAX_FILES` index entries at the
256-byte path limit, so those limits bind first. The client parses it once when
it admits the bundle.

`cinnabar-cxb build --experience <experience.toml> --component <guest.wasm>
--publisher-seed <seed> --out <id>.cxb` reads identity/version and the `[client]`
table, componentizes the guest, signs and verifies the archive. `keygen <seed>`
creates a private publisher seed. `seed-cache --cxb <id>.cxb --user-data <dir>`
preloads a verified bundle. Cache reads rehash bytes; cache entries carry no grants.

For a low-level local-server deployment, supply `-extension-key <seed>`,
`-extension-audience <host:port>` and `-extension-cxb <dir>` together. The audience
must exactly match the address players join. The local server advertises a cache-only
bundle origin; seed each client's cache. Offers expire, so restart at least daily.
Paired `mod run` prepares keys, installation and cache automatically.

## Versions and wire

| Axis | Contract/source |
| --- | --- |
| Handshake API | `API_VERSION` in [policy.rs](../crates/server-experience/src/policy.rs) |
| Envelope versions | `WIRE_VERSION` through `MAX_WIRE_VERSION` in policy; v2 negotiates fragmentation and ceilings |
| Client WIT | [capabilities.wit](../crates/experience-sdk/wit/client/deps/server-experience/capabilities.wit), currently 1.5; older 1.x components still link |
| Exported world | [server-bundle](../crates/mod-api/wit/client.wit) |
| Canonical fields/domains/limits | [extension.json](../crates/server-experience/schema/extension.json) |

Transport uses Bedrock `ScriptMessage` with `message_id = "cinnabar:extensions/v1"`.
Both packet-header subclient routes and the envelope must agree; only route zero
is supported. No marker means no extension probe. Before consent, extension
messages are dropped before world sequencing.

Signed payloads use exact compact serialization of the versioned Rust structs,
not JCS. Keys, signatures, digests and nonces are lowercase hex. Preserve field
and array ordering; sign the document's domain bytes plus canonical payload.
Use [generated document types](../tools/localserver/cinnabarext/doc.go) and
`testdata` fixtures for another language's signer, rather than reconstructing
encoding from prose. `cinnabar-cxb write-fixtures <dir>` regenerates the Rust fixtures.

Wire v2 keeps client state on world-epoch changes and fragments oversized records
within negotiated limits. Sequences, routes, schemas, epochs, reassembly budgets
and rates are checked. Wire v1 falls back on dimension change. Neither version's
fallback may depend on a final message arriving from a stopped client.

Server-driven transfers, including same-dimension moves, use an `epoch` notice.
The client replies with `world_ack`: `requested_epoch` echoes the notice and
`world_epoch` is a strictly newer client epoch. Deploy matching client/helper
revisions. The helper fences both directions before delivery; input must match the
acknowledged epoch. Client state stays live and `epoch` requests fresh state.

Committed admissions queue per connection and publish in order at activation or
commit. Acknowledgment retires that notice and all earlier admissions; traffic
opens only with an empty queue and no preparation. Aborting preparation preserves
committed admissions. A client part activating during preparation joins the fence;
after commit it receives the destination notice.

Native epoch updates consume ordinary data credits and wait ahead of later sends.
Queued notices and matching acknowledgments bypass data credits. Notices must fit
256 encoded bytes and exceed the last acknowledged notice; equal/older notices
are dropped. The client advances its acknowledged epoch before writing the reply.
Multiple notices can precede one cumulative acknowledgment. Retired/repeated
acknowledgments use ordinary credits and leave the fence unchanged. Failed notice
writes end the optional session without a retry timer; travel remains committed.
The fence does not certify that terrain has rendered.

## Developer media

`media.control` names an indexed descriptor; a scene quad whose texture is that
descriptor path displays it. Media callbacks use `cinnabar.media` records with
path, event (`playing`, `paused`, `stopped`, `ended`) and position in milliseconds.
The decoder profile is progressive WebM with AV1/Opus, 8-bit I420, limited-range
BT.709, bounded to 720p/30 fps and mono/stereo 48 kHz. Signed chunk hashes are
checked before decode. Whole-object reconciliation after streaming is incomplete.

Build `mod-host` with `media` for the decoder helper. Windows execution is currently
unavailable. Local HTTPS media requires `CINNABAR_DEV_MEDIA_CA` naming the server's
CA and the developer execution switch. Localserver's `-extension-media <dir>` and
`-extension-media-addr <loopback:port>` serve files and write the CA under the world.
`cinnabar-cxb media` strips WebM tags and writes a descriptor.

Playback uses the client media clock. Server-clock wire routing, seamless loops,
sparse seeking and UI/entity surfaces are incomplete; seek decodes from the start.
Use ordinary posters, captions and controls when media is unavailable.
