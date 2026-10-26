# Camera and rendering

This guide covers the camera policies, the world-space drawing interfaces and the terrain reads behind them. Camera work splits across two interfaces: `gameplay` for reads and rotation, `camera` for per-frame view policy. Rendering uses `render` for immediate primitives and retained post-process passes, the world-draw `drawing` interface for retained geometry, and `masks` for retained actor silhouettes.

## Camera reads and rotation

`gameplay::read_frame` returns the current input-owned frame: session, dimension, eye position, yaw and pitch in actor YXZ radians (yaw left, pitch up), whether attack is held, frame seconds, and remote players' feet positions without names. It needs the `players` or `terrain` grant, and player rows need `players`. `gameplay::read_camera` returns the last rendered camera pose, including third-person and rig offsets. Both return none when gameplay does not own input — in a menu, for instance — so treat none as "not now", not as an error.

`gameplay::rotate(yaw_delta, pitch_delta)` turns the view, in radians. It needs the `camera` grant and a current frame, commits only after the frame returns, and the accumulated change per axis in one callback is capped at `MAX_CAMERA_DELTA_RADIANS` (0.25).

Conventions differ by interface on purpose: observations such as `player_state::read_motion` report degrees, while `gameplay` reports radians. `mod_api::bedrock_view_angles(yaw, pitch)` converts observed actor YXZ degrees to Bedrock view degrees.

## Per-frame camera policy

The `camera` interface is opt-in policy that expires unless renewed. All of it needs the `camera` grant.

| Function | Behaviour |
| --- | --- |
| `set_view_tilt` | Adds `roll` and `pitch`, each at most `MAX_VIEW_TILT_DEGREES` (90), to the first-person view where native damage shake applies; `suppress_hurt_shake` removes that shake. Positive pitch turns up. Current frame only, including while a menu owns input; none, omission, unload and failed callbacks restore the native view |
| `read_damage_shake` | The native damage shake of the last presented view at full strength, in the same degrees, before the player's damage-shake setting or any mod tilt — so a suppressing tilt can redraw it at another intensity |
| `set_preserve_teleport_rotation` | Renew every frame: only actual teleports preserve yaw and pitch. Omission, no gameplay frame or a failed callback restores server rotation |
| `set_view_scale` | Current-frame FOV and look multipliers, bounded to 0.1..1 and 0.05..1 (`MIN_VIEW_FOV_SCALE`, `MIN_VIEW_LOOK_SCALE`). Renew only during focused, input-owned gameplay; omission restores both without touching saved settings |

`gameplay::set_camera_rig` retains a third-person rig: a camera-local offset (x right, y up, z back), roll in radians and FOV change in degrees, swept against blocks; none restores the player's own view. The bounds are `MAX_RIG_SIDE_BLOCKS` (2.0), `MAX_RIG_VERTICAL_BLOCKS` (2.0), `MAX_RIG_BACK_BLOCKS` (8.0), `MAX_RIG_ROLL_RADIANS` (0.6) and `MAX_RIG_FOV_DELTA_DEGREES` (30.0).

## Post-process passes and primitives

Immediate primitives need `render` or `world_draw`. Post-process pass calls need `render` or `render_post`, and reading scene depth also needs `render_depth`. These grants apply within the current world's drawing policy.

`register_pass` validates a WGSL pass: a `pass_spec` with a name, an order, a depth flag, and source defining `fn effect(uv: vec2<f32>) -> vec3<f32>` against the host prelude. Passes run after the scene and before the HUD, ordered by `order` then name. `update_pass` sets a retained enable flag and up to `MAX_PASS_PARAMS` (16) floats exposed to the shader as `param(i)`, and `remove_pass` unregisters. `examples/mods/render-sample` registers a vignette this way and toggles it per frame.

`draw` appends world-space primitives — decals, ribbons, beams and billboards — and `draw_lines` appends depth-tested lines. A successful `init` or `frame` publishes a replacement drawn set, so redraw everything you want visible, every frame. Event callbacks do not publish immediate primitives by themselves.

| Limit | Value | Constant |
| --- | --- | --- |
| Passes per mod | 8 | `MAX_RENDER_PASSES` |
| Render and retained drawing calls per callback | 32 | `MAX_RENDER_CALLS` |
| Shader bytes | 16 KiB | `MAX_SHADER_BYTES` |
| Bytes in a pass name | 32 | `MAX_PASS_NAME_BYTES` |
| Decals / ribbons / beams / billboards across a frame's draw calls | 64 / 32 / 16 / 512 | `MAX_RENDER_DECALS`, `MAX_RENDER_RIBBONS`, `MAX_RENDER_BEAMS`, `MAX_RENDER_BILLBOARDS` |
| Points in one ribbon | 64 | `MAX_RIBBON_POINTS` |
| Largest primitive extent | 64 blocks | `MAX_PRIMITIVE_EXTENT_BLOCKS` |

One shader validation is allowed per frame callback (`MAX_PASS_COMPILES_PER_FRAME`); `init` may compile every pass, so register there.

## Retained world drawing

The `drawing` interface (`cinnabar:world-draw/drawing`) keeps geometry alive across frames under the `world_draw` grant, with separate grants for what the geometry can do: `entity_observe`, `entity_highlight`, `render_through_walls`, `render_material` and `render_post`. Check `drawing::capabilities()` and handle the typed `draw-error` rather than assuming a grant.

The flow is handles all the way down: `create_geometry` uploads a mesh or polyline and returns a handle, `create` places an instance of it at a world, block, camera or entity anchor, `update` moves it, `remove` deletes it, and `replace_frame_group` swaps a named group for callback-lifetime output. Remove instances before their geometry. Retained budgets per owner: `MAX_DRAW_INSTANCES` (1024) instances, `MAX_DRAW_GEOMETRIES` (256) geometries, `MAX_DRAW_VERTICES` (65,536) vertices, `MAX_DRAW_INDICES` (196,608) indices and `MAX_DRAW_MATERIALS` (8) custom materials.

The `mod_api::drawing::shapes` helpers build geometry records for you — `box_wireframe`, `circle`, `sphere_wireframe`, `arrow`, `polyline`, `path_gradient`, `grid`, `chunk_border` — and `mod_api::drawing::instance` fills an instance with depth testing, premultiplied blending and identity placement as defaults. A minimal use:

```rust
use mod_api::drawing::{api, instance, shapes};

let shape = shapes::circle(2.0, 32, [0.2, 0.6, 1.0, 1.0]).expect("valid circle");
let geometry = api::create_geometry(&shape[0]).expect("drawing geometry admitted");
let mut draw = instance(geometry, api::Anchor::World(api::WorldAnchor {
    dimension: 0,
    origin: (0.0, 64.0, 0.0),
}));
draw.width = 2.0;
draw.width_mode = api::WidthMode::Pixels;
let drawing = api::create(&draw).expect("drawing instance admitted");
```

Keep both handles in your state and `update` the instance to move the drawing instead of rebuilding geometry. Handles belong to one component and session: session changes, reload, revocation and dimension transfers invalidate them. Explicit instances persist until removed or revoked; frame groups expire on the next successful callback.

The `masks` interface builds retained union silhouettes of actors under `entity_highlight`: resolve an actor with `entity`, `create` a mask, `select` its members, `set_style` its outline and glow, and optionally `register_effect` a post effect sampling it, which also needs `render_post`. The `entity` lookup additionally needs `entity_observe` and `render_through_walls`; masks that ignore depth also need `render_through_walls`. At most `MAX_MASKS` (8) masks with `MAX_MASK_ENTITIES` (32) actors each; both constants are in `mod_api::drawing`.

## Terrain

The `terrain` interface reads bounded local terrain under the `terrain` grant. Unknown terrain is never reported as air.

`read_region` takes a `region_query` — inclusive minimum, exclusive maximum, and requested `collision`/`selection` geometry flags — and returns `pending`, `unchanged(stamp)` or `changed(region)`. A query your callback makes becomes a subscription the client samples before later frames, so a new query reads `pending` first; pass the stamp you last received to skip unchanged data. Stamps compare only within the same query, session, dimension and registry generation.

The region's cells are indexed `((x * size-z) + z) * size-y + y` relative to the minimum; the `loaded` bitfield marks known cells, `states` holds a palette index per layer slot (65535 means no layer, never air), and the palette carries each block's runtime id, identifier, canonical state, tags, physics and shape bindings. Shape 0 is known-empty; a shape binding may be `not-requested`, `fixed` or `per-cell`, and a resolved shape may be `unavailable` when the facts a geometry would need are missing.

`find_blocks` returns positions of loaded blocks matching 1 to `MAX_FOUND_BLOCK_IDENTIFIERS` (8) namespaced identifiers, nearest first. A search is a subscription too: a new search returns nothing, and results fill in as loaded terrain is scanned between frames. Searches and region queries share the query budget and the callback's terrain call budget.

| Limit | Value | Constant |
| --- | --- | --- |
| Extent per axis | 16 blocks | `MAX_TERRAIN_EXTENT` |
| Cells per query | 4096 | `MAX_TERRAIN_CELLS` |
| Queries per mod per frame | 4 | `MAX_TERRAIN_QUERIES` |
| Distance from the player's feet, per axis | 128 blocks | `MAX_TERRAIN_DISTANCE` |
| Results per search | 1024 | `MAX_FOUND_BLOCKS` |

Errors are typed: `denied`, `unavailable`, `invalid-query`, `out-of-range`, `budget-exhausted` or `limit-exceeded`. On `limit-exceeded`, shrink the query. Sampling is host work, but your own pathfinding and caching over the result still spend frame fuel, so work through a large region incrementally with `budget::remaining-fuel`, as [How mods run](how-mods-run.md) describes.

See the [camera](../reference/camera.md), [gameplay](../reference/gameplay.md), [render](../reference/render.md), [terrain](../reference/terrain.md) and [drawing](../reference/world-draw-drawing.md) reference pages for exact types.
