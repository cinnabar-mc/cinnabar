//! Piston head motion between the extensions the server reports.
//!
//! A head moves half a block per game tick toward its state's end, from the last reported
//! extension. Motion is forgotten once a frame stops resolving its block.

use bevy::platform::collections::HashMap;
use render_model::{BlockEntityKind, PistonArmModel};

use super::{
    super::describe::{Template, piston_arm},
    BlockEntityRuntime, PhysicsCollisionRegistries, block_info,
};

/// The actor data id of a piston head.
const ACTOR_ID: &str = "PistonArm";

/// Extension per second: half a block per tick at twenty ticks per second.
const EXTENSION_PER_SECOND: f64 = 10.0;

/// A retracted head for a piston block, drawn from the palette until actor data arrives.
pub(super) fn resting(
    runtime: &mut BlockEntityRuntime,
    collisions: &PhysicsCollisionRegistries,
    mode: assets::NetworkIdMode,
    runtime_id: u32,
) -> Option<BlockEntityKind> {
    let info = block_info(runtime, collisions, mode, runtime_id)?;
    if info.backing?.entity.renderer != assets::facts::BlockEntityRenderer::PistonArm {
        return None;
    }
    let Template::PistonArm { facing, sticky, .. } = piston_arm(&info.name, &info.state, None)?
    else {
        return None;
    };
    Some(BlockEntityKind::PistonArm(PistonArmModel {
        facing,
        sticky,
        progress: 0.0,
    }))
}

/// Whether `block` of `chunk` holds the actor data that drives its piston head.
pub(super) fn has_actor(chunk: &world::Chunk, dimension: i32, [x, y, z]: [i32; 3]) -> bool {
    chunk
        .block_entity(world::BlockEntityKey::new(dimension, x, y, z))
        .is_some_and(|nbt| nbt.id() == Some(ACTOR_ID))
}

/// Where a piston head's world light is read from; other kinds light from elsewhere.
pub(super) fn light_sample(kind: &BlockEntityKind, block: [i32; 3]) -> Option<[f32; 3]> {
    match kind {
        BlockEntityKind::PistonArm(model) => Some(model.plate_center(block)),
        _ => None,
    }
}

/// The head a piston template shows at `now`.
pub(super) fn resolve(
    template: &Template,
    position: [i32; 3],
    now: f64,
    runtime: &mut BlockEntityRuntime,
) -> Option<BlockEntityKind> {
    let &Template::PistonArm {
        facing,
        sticky,
        extension: reported,
        target,
    } = template
    else {
        return None;
    };
    let progress = extension(&mut runtime.piston_motions, position, reported, target, now);
    Some(BlockEntityKind::PistonArm(PistonArmModel {
        facing,
        sticky,
        progress,
    }))
}

/// One head moving from a reported extension.
#[derive(Clone, Copy)]
pub(super) struct PistonMotion {
    start: f32,
    target: f32,
    started: f64,
    seen: f64,
}

/// The extension a head with this report shows at `now`.
pub(super) fn extension(
    motions: &mut HashMap<[i32; 3], PistonMotion>,
    position: [i32; 3],
    start: f32,
    target: f32,
    now: f64,
) -> f32 {
    if start == target {
        motions.remove(&position);
        return target;
    }
    let motion = motions.entry(position).or_insert(PistonMotion {
        start,
        target,
        started: now,
        seen: now,
    });
    if motion.start != start || motion.target != target {
        (motion.start, motion.target, motion.started) = (start, target, now);
    }
    motion.seen = now;
    let travelled = ((now - motion.started).max(0.0) * EXTENSION_PER_SECOND) as f32;
    if target > start {
        (start + travelled).min(target)
    } else {
        (start - travelled).max(target)
    }
}

/// Drops the motion of every head this frame did not resolve.
pub(super) fn prune(motions: &mut HashMap<[i32; 3], PistonMotion>, now: f64) {
    motions.retain(|_, motion| motion.seen == now);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_head_moves_half_a_block_per_tick_and_stops_at_its_end() {
        let mut motions = HashMap::new();
        let at = |motions: &mut _, now| extension(motions, [0; 3], 0.0, 1.0, now);
        assert_eq!(at(&mut motions, 10.0), 0.0);
        assert_eq!(at(&mut motions, 10.05), 0.5);
        assert_eq!(at(&mut motions, 10.5), 1.0);
    }

    #[test]
    fn a_new_report_restarts_from_its_own_extension() {
        let mut motions = HashMap::new();
        extension(&mut motions, [0; 3], 0.0, 1.0, 10.0);
        assert_eq!(extension(&mut motions, [0; 3], 1.0, 0.0, 11.0), 1.0);
        assert_eq!(extension(&mut motions, [0; 3], 1.0, 0.0, 11.05), 0.5);
    }

    #[test]
    fn a_settled_head_keeps_no_motion_and_unseen_motion_is_dropped() {
        let mut motions = HashMap::new();
        extension(&mut motions, [0; 3], 0.0, 1.0, 10.0);
        extension(&mut motions, [1; 3], 0.0, 1.0, 11.0);
        prune(&mut motions, 11.0);
        assert_eq!(motions.len(), 1);
        assert_eq!(extension(&mut motions, [1; 3], 1.0, 1.0, 12.0), 1.0);
        assert!(motions.is_empty());
    }
}
