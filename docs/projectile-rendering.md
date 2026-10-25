# Projectile rendering investigation

The fixes in `fix/projectile-render` address invisible item sprites, undersampled arrow
textures, discarded arrow plane backs, duplicated world yaw, and dropped remote motion.
They do not close the vanilla projectile parity gate.

## Vanilla rules

| Rule | Behaviour |
| --- | --- |
| Face UV defaults | Face dimensions are initialized from caller defaults before an optional `uv_size` read. Omitted face UV dimensions use the authored cube face size, shared between UV validation and mesh generation. |
| Camera rotation | `query.camera_rotation(0/1)` reads camera pitch/yaw. Billboard bones retain the pack's camera rotation, without an additional mob body yaw. |
| Actor yaw | Four actor types read wrapped, interpolated absolute actor yaw; mobs read bounded relative yaw. Arrow target yaw is absolute, and world placement does not apply it again. |
| Actor classification | Actor registrations identify arrow `0xc00050`, fireworks rocket `0x48`, wither skull `0x400059`, dangerous wither skull `0x40005b`. One classification supplies both query evaluation and world placement. |
| Interpolation | The classic movement route starts interpolation with at least three steps. Existing three-tick interpolation is retained. Projectile prediction/component selection still needs confirmation. |
| Motion | Throwable and abstract-arrow motion handlers replace retained velocity. The arrow initializes zero prior pitch/yaw from its motion vector. Remote `SetActorMotion` reaches the store; an unrotated arrow initializes its launch angles without moving its position. |

The pinned resource pack is selected by `assets/vanilla-source.json`. Its
`models/entity/item_sprite.geo.json` declares an 8×8 UV frame on an 8×8×0 cube,
origin `[-4,-2,0]`: 0.5 blocks square, from -0.125 to 0.375 blocks vertically.
The entity definition directly binds its item icon, such as
`textures/items/ender_pearl`, rather than an inventory atlas lookup. The actual
icon raster can be 16×16. The compiler previously excluded item texture paths,
required equal declared/raster dimensions, and rejected the unused box UV envelope.
These were independent barriers to publishing the sprite artwork.

The pack's arrow geometry has two crossed shaft planes and a cap. Shaft face UVs
omit `uv_size`; their default region is 16×5 texels. The arrow animation supplies
pitch/yaw and axis scale `[0.7,0.7,0.9]`. Item sprites use
`animation.actor.billboard`; the compiler continues to load the pack's geometry,
texture selection, and animation rather than synthesizing replacements.

The neutral catalog profile already declares two-sided rendering. Its mesh builder
now supplies front UVs for a missing back face, while retaining explicit opposing
UVs and leaving the separate skin mesh builder unchanged. This repairs arrow backs
within that profile. Exact target arrow material and lighting remain unverified.

## Verification

Regression tests were executed against unfixed production code and observed failing
before each corresponding implementation change. They cover item icon admission,
raster resolution, sprite UV bounds, default arrow face UV size, arrow back UVs,
absolute arrow target yaw, duplicate world yaw, remote motion ingress, and initial
arrow motion rotation. Later motion must retain the existing orientation and position.

`projectile_report::render_projectile_states` uses the real compiled vanilla geometry,
artwork, actor poses, presentation adapter, and scene publication. `scene_report` then
rasterizes the published triangles offline, including backface UV selection. The
fixed-camera gallery covers arrow flight, an arrow at a textured stone block, pearl,
and snowball. This is a CPU frame witness, not target-platform GPU acceptance.

Rebuild the actor carrier with `make assets` before using the fixes with previously
compiled local assets. This work does not write the owner's `.local` directory.

## Open parity checks

- Exact projectile network component selection: the classic movement route has a
  three-step minimum, matching the existing default. Predicted movement uses a
  separate buffer and actors without an interpolator can snap; projectile-specific
  selection is not yet confirmed.
- Frame-level rotation fidelity: the shared GPU rig interpolates posed matrices;
  vanilla target queries interpolate angles. Dedicated projectile frame validation
  remains open.
- Arrow collision/stuck-state transition and shake trigger/countdown. The pack shake
  animation exists; retained client shake state is not implemented. Configuration
  parsing is not proof of the runtime hit trigger.
- Tipped-arrow rendering: the pack controller has no tint expression, but builtin
  tint or particle behavior has not been excluded with target client evidence.
- Initial AddActor velocity-only launch orientation and special arrow facing flags;
  the measured SetActorMotion initialization is implemented.
- Potion glint, exact target material/lighting, and native target-platform frames.
- The unused sprite back box UV samples outside the declared width; the pack's
  billboard exposes the correctly textured front. Exact backside behavior is open.

No live server connection was used and no parity gate was marked complete.

## Thrown trident

Vanilla's client entity for `minecraft:thrown_trident` names only its textures and
geometry; a native renderer draws it. The renderer loads the model's `pole` bone,
translates to the actor's interpolated render position, turns by `yaw - 90` degrees about
Y and then by `pitch + 90` degrees about Z, and draws at the actor's scale over 16.
Native model parts load authored heights as `24 - y`, so the head of the pack's pole
leads along the projectile rotation (`yaw = atan2(x, z)`, `pitch = atan2(y, horizontal)`).
An enchanted trident (actor flag `Enchanted`, set from the thrown item's foil) adds the
item glint at UV scale 0.5.

The pack compiler gives a controllerless thrown trident a controller that selects its
default geometry and texture. Client presentation places that rig with the native
transform, keeps rotation-invariant culling bounds and applies the item foil. The
trident is lit at the actor's position, which it turns about, rather than at the rig
origin trailing 1.5 blocks behind it; an upright trident would otherwise sample the block
below and draw black.

While the owner (the `Owner` actor id) is present, the native renderer also draws the
loyalty rope back to it as two triangle strips of 37 segments, textured with the 7x1
`textures/entity/loyalty_rope`. Vertex pairs `0..=min(2 * returning ticks + 1, 37)` are
drawn, where the trident counts the ticks its `ReturnTrident` flag is set and never resets
them. A trident that has not returned therefore shows a one-segment stub. At `t = i / 37`:

- The span runs from the trident's render position by
  `owner * a + owner_prev * (1 - a) - (trident * a + owner_prev * (1 - a))`, with the
  owner end lowered by 0.5. This blends the trident with the owner's previous position.
- The sag is `(1 - (2t - 1)^2) * min(|span| / 30, 0.5)`.
- The coil phase is `t * 8π + (unique id + tick count + a) * -0.1`, with the id and tick
  count summed as 32-bit unsigned values.
- The coil turns by the owner yaw `prev + (yaw - prev) * 0.5 * a`.
- Each vertex is `((cos θ - sin θ) sin φ sag, sag + 0.1 + cos φ * r, sin φ (cos θ + sin θ) sag)`
  plus `span * t`, with `r = 0.02` for the first strip and `0.01` for the second.
- The second vertex of each pair is offset by `0.1 * sag` on X and Y (first strip) or on
  X and Z (second strip).
- `u = (i % 7) / 7` and `v = 1`. Sines use the native 65536-entry table.

Cinnabar draws the rope in the block-entity pass, lit like the trident.

Open checks:

- The rope's `entity_loyalty_rope` material is unverified; it draws alpha-tested and
  unculled. A pack that renames the entity's `loyalty_rope` texture is not followed.
- The native material behind the model is unverified; the generated binding uses
  `entity_alphatest` when the entity names none. Every authored bone draws, not only `pole`.
- Abstract-arrow motion seeding (`lerpMotion`) applies to tridents too; Cinnabar seeds
  only the arrow class.
- The native renderer's light sample point is unverified.

The charge pose is native too. The pack's third-person player state plays
`brandish_spear`, gated on `variable.is_brandishing_spear` and posed by
`variable.charge_amount`, which no pack script assigns. Vanilla's mob-variable update
sets them every tick: an actor using a main-hand item with the spear use animation (the
trident's) brandishes with `charge_amount = clamp(elapsed use ticks / 10, 0, 1)`, and a
non-player mob holding a trident it is not using brandishes while its synched target id
is not 0. The paper doll and first-person states do not play `brandish_spear`.
