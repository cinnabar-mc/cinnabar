//! Cell overlays under the interaction ray: each block-state overlay of an overlay host picks by
//! its state's selection boxes turned to its face, as the server's `target` rule turns them, and
//! the nearest hit wins; at an equal distance an overlay wins over its host, and the lower slot
//! over the higher.

use block_transform::overlay_face::FaceTurn;
use world::{BlockEntityKey, OverlayLook};

use super::{Candidate, PaletteWorld, WorldQueryError, candidate_precedes, ray_box};
use crate::{Aabb, Vec3};

/// The cell overlay a [`super::BlockHit`] met: its slot, and its turned selection box in world
/// space, which the selection outline draws instead of the host's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OverlayHit {
    pub slot: u8,
    pub outline: Aabb,
}

impl PaletteWorld<'_> {
    /// Offers the ray's hits on the overlays of the overlay host at `block` as candidates.
    pub(super) fn inspect_overlays(
        &self,
        block: [i32; 3],
        (origin, direction, max_distance): (Vec3, Vec3, f64),
        best: &mut Option<Candidate>,
    ) -> Result<(), WorldQueryError> {
        let Some(ids) = self.overlay_ids else {
            return Ok(());
        };
        let key = BlockEntityKey::new(self.dimension, block[0], block[1], block[2]);
        let Some(overlays) = self
            .store
            .block_entity(key)
            .and_then(|nbt| nbt.cell_overlays().cloned())
        else {
            return Ok(());
        };
        let host = self.primary_runtime_id(block)?;
        let resolved = overlays.resolve(ids);
        let Some(on) = resolved.on(host) else {
            return Ok(());
        };
        let offset = Vec3::new(
            f64::from(block[0]),
            f64::from(block[1]),
            f64::from(block[2]),
        );
        for overlay in on {
            let (OverlayLook::Block(state), Some(turn)) =
                (&overlay.look, FaceTurn::of(overlay.face))
            else {
                continue;
            };
            let Some(physics) = self.registry.physics(*state) else {
                continue;
            };
            if !self.may_pick(physics) {
                continue;
            }
            let shapes = physics.pick_shapes.as_deref().unwrap_or(&physics.shapes);
            for (shape_index, shape) in shapes.iter().enumerate() {
                let outline = turned(*shape, turn).translated(offset);
                let Some((fraction, distance, face, point)) =
                    ray_box(origin, direction, max_distance, outline)
                else {
                    continue;
                };
                let local = point - offset;
                let candidate = Candidate {
                    block_pos: block,
                    face,
                    hit_local: Vec3::new(
                        local.x.clamp(0.0, 1.0),
                        local.y.clamp(0.0, 1.0),
                        local.z.clamp(0.0, 1.0),
                    ),
                    hit_world: point,
                    runtime_id: host,
                    distance,
                    fraction,
                    shape_index,
                    overlay: Some(OverlayHit {
                        slot: overlay.slot,
                        outline,
                    }),
                };
                if best
                    .as_ref()
                    .is_none_or(|previous| candidate_precedes(&candidate, previous))
                {
                    *best = Some(candidate);
                }
            }
        }
        Ok(())
    }
}

/// `shape`, in block units of a cell whose corner is the origin, turned about the cell's centre.
fn turned(shape: Aabb, turn: FaceTurn) -> Aabb {
    let centred = |point: Vec3| turn.turn([point.x - 0.5, point.y - 0.5, point.z - 0.5]);
    let [a, b] = [centred(shape.min), centred(shape.max)];
    let corner = |pick: fn(f64, f64) -> f64| {
        Vec3::new(
            pick(a[0], b[0]) + 0.5,
            pick(a[1], b[1]) + 0.5,
            pick(a[2], b[2]) + 0.5,
        )
    };
    Aabb::new(corner(f64::min), corner(f64::max))
}
