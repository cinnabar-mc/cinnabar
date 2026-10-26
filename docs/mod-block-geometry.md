# Block geometry and mesh clipping

Rendering integration is incomplete. The contract and resource admission exist,
but block geometry and clipped instances are not yet rendered. The API is not
ready for use.

## Block grids

`cinnabar:world-draw/drawing@0.2.0` adds `geometry-spec.blocks`. A grid has a
size, a palette and dense cells. Cells use x,z,y order: y changes fastest, then
z, then x. Each cell selects a palette entry and optional independent sky and
block light values. Light values must be between 0 and 15. Missing sky light
defaults to 15; missing block light defaults to 0. Outside cells are air.

Palette entries are either current-session runtime IDs or block names with a
JSON state object. Named entries must resolve completely; an unknown state is
an error. Runtime IDs must not be carried into a different connection.
Creating or drawing a grid requires both `world_draw` and `render_material`.
Supplying a grid does not grant access to the live world's blocks.

The CPU implementation uses the terrain mesher, including neighbour culling,
models, liquid geometry and solved light sampling. `geometry-status` reports
pending work, readiness or a typed worker error. `update-blocks` changes cells
without changing the palette or size. Repeated edits of one cell use the last
value. Unchanged edits preserve resource identity; changed edits share sections
outside their dependency columns. Another batch while meshing is pending returns
`in-use`. Updates commit atomically with the callback.

Grid, palette, state-string, triangle and memory limits are declared in
[`mod_api::drawing`](../crates/mod-api/src/drawing.rs). Existing retained geometry
and callback byte limits also apply. Rejection returns `budget-exceeded`.

## Clip resources

`create-clip` retains triangle geometry with its own anchor and transform.
Instances reference this resource through `instance.clip`. A clip can name a
parent clip; the bounded parent chain describes intersected coverage. Lines and
block grids cannot serve as masks. Handles remain scoped to their owner and
connection. Removing referenced clips or mask geometry returns `in-use`.
Actor despawns remove their anchored clips and dependent draws.

The intended rendering contract uses visible mask coverage against world depth
and a separate depth buffer for its contents. Implementing this rendering path,
the depth/blend composition rules and GPU attachment admission remains open.
The current renderer suppresses clipped instances.

## Migration from 0.1

Rebuild components against the updated SDK. The world-draw package imports,
including `masks` and `assets`, now use version 0.2. Existing instance records
need `clip: none`; Rust's `drawing::instance` supplies that default. Existing
triangle and line geometry otherwise keep their record shapes and grants.
