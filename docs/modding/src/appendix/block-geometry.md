# Block geometry and mesh clipping

Use retained block grids for textured terrain previews, and mesh clips to restrict
any retained drawing to a visible screen region. These capabilities do not read
another dimension or supply portal behaviour: the mod supplies cells and placement.

## Block grids

`cinnabar:world-draw/drawing@0.2.0` adds `geometry-spec.blocks`. A grid has a
size, a palette and dense cell words. Cells use x,z,y order: y changes fastest,
then z, then x. Use `drawing::pack_block_cell` to encode a palette entry and
optional independent sky and block light values. Plain words cross the component
boundary without a guest-side record conversion loop; prepare large grids during
`PlayerMod::init`, then submit them when the world becomes available. The legacy
extension entry point has only the regular frame fuel allowance during initialization.
Light values must be between 0 and 15. Missing sky light
defaults to 15; missing block light defaults to 0. Outside cells are air.

Palette entries are either current-session runtime IDs or block names with a
canonical JSON state object, using the typed values returned by terrain observations.
For example, lava uses `{"liquid_depth":{"type":"int","value":0}}`.
Named entries must resolve completely; an unknown state is
an error. Runtime IDs must not be carried into a different connection. A pack registry or
meshing-settings change invalidates a completed grid with `unsupported`; recreate
it, or submit a cell update to request a fresh generation in the new context.
Creating or drawing a grid requires both `world_draw` and `render_material`.
Supplying a grid does not grant access to the live world's blocks.

The host uses the terrain mesher, packed GPU streams, atlas, biome fallback,
lighting tables and texture animation clock. This includes models, liquids and
neighbour culling. Materials and tint remain host-owned for block grids: leave
`material` empty and `tint` white. Procedural materials still apply to triangle
and line geometry. `geometry-status` reports
pending work, readiness or a typed worker error. `update-blocks` changes cells
without changing the palette or size. Repeated edits of one cell use the last
value. Unchanged edits preserve resource identity; changed edits share sections
outside their dependency columns. Another batch while meshing is pending returns
`in-use`. Updates commit atomically with the callback.

Grid, palette, state-string, triangle and memory limits are declared in
[`mod_api::drawing`](https://github.com/cinnabar-mc/cinnabar/blob/dev/crates/mod-api/src/drawing.rs). Existing retained geometry
and callback byte limits also apply. Rejection returns `budget-exceeded`.
Client parts also share their scene triangle allowance with legacy objects,
immediate primitives, retained placements and mask chains. Completed grid meshes
are checked with every placement before rendering; asynchronous overflow is
reported through `geometry-status`.

## Clip resources

`create-clip` retains triangle geometry with its own anchor and transform.
Instances reference this resource through `instance.clip`. A clip can name a
parent clip; the bounded parent chain describes intersected coverage. Lines and
block grids cannot serve as masks. Handles remain scoped to their owner and
connection. Removing referenced clips or mask geometry returns `in-use`.
Actor despawns remove their anchored clips and dependent draws.

Each mask tests world depth at its own surface, including individual MSAA samples.
A parent intersects coverage. The host clears private depth for each clip, so
contents test and write depth against other contents of that clip and can draw
behind the world visible through the surface. Foreground world geometry still
hides the surface. Clips do not write their contents' depth into the world.

`depth.test` uses the clip's private depth; `depth.always` bypasses that comparison
but keeps mask coverage and its world-depth test. Existing through-wall grants
still apply to `always`. Triangle and line contents write private depth and keep
their premultiplied or additive blend. Block grids retain terrain's opaque,
cutout and sorted transparency passes; additive selects additive composition.
Different clips have independent depth, and overlapping clips compose in handle
creation order. A nested clip intersects its parent's mask, rather than reading
its parent's content depth.

`clip-status` reports attachment readiness or an explicit budget error. Check
both the grid and clip status when handling a large preview. Renderer admission
failures remain errors on that resource; recreate it after changing the view or
freeing resources. Pending resources do not expose incomplete uploads.

Meshing, GPU arena uploads and transparency sorting run on bounded worker lanes.
Steady geometry reuses cached buffers and sort orders. Camera or anchor changes
update only placement uniforms and, when needed, transparent face order. Retired
GPU storage stays charged until submitted work completes. Client parts share the
same API; completion feedback is fenced to the exact grid generation.

## Example

`examples/mods/portal-view` supplies a fixed 48×64×48 grid with netherrack and lava,
places it behind a four-by-five triangle mask, and draws an obsidian frame.
Build and pack it with its explicit permission request:

```sh
cargo build --locked --release --target wasm32-unknown-unknown -p portal-view-mod
cargo run --locked -p mod-host --bin mod-host -- pack \
  --grants examples/mods/portal-view/grants.json \
  target/wasm32-unknown-unknown/release/portal_view_mod.wasm /tmp/portal-view.wasm
```

Select `/tmp/portal-view.wasm` using `CINNABAR_MOD_COMPONENT`.
Set `CINNABAR_MOD_PLAYERS=1`, `CINNABAR_MOD_WORLD_DRAW=1`,
`CINNABAR_MOD_RENDER_MATERIAL=1` and `CINNABAR_MOD_HUD=1`. Join a local world;
the portal is placed ahead of the initial camera. F8 hides and restores its
contents while retaining the grid. No per-frame guest meshing is performed.

## Migration from 0.1

Rebuild components against the updated SDK. The world-draw package imports,
including `masks` and `assets`, now use version 0.2. Existing instance records
need `clip: none`; Rust's `drawing::instance` supplies that default. Existing
triangle and line geometry otherwise keep their record shapes and grants.
