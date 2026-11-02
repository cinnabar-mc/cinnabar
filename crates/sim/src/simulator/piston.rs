//! Piston arms pushing the player.

use super::{
    MovementInput, MovementMode, PlayerState, SimulationError, collision::resolve_motion,
    scaffolding::ScaffoldingView,
};
use crate::{CollisionWorld, WorldCollisionIdentity};

/// Moves the player clear of each piston arm that moves on the update of `next.tick`, and
/// returns the identity of the terrain read. A rider moves with its vehicle instead.
///
/// The world still shows the preceding tick, as native
/// `PistonBlockActor::moveEntityLastProgress` restores the arm's last progress while it moves
/// the pushed actor with `Actor::move`, so the player is not embedded in the arm it leaves.
pub(super) fn push(
    world: &impl CollisionWorld,
    next: &mut PlayerState,
    input: MovementInput,
) -> Result<Option<WorldCollisionIdentity>, SimulationError> {
    if input.mode == MovementMode::Riding {
        return Ok(None);
    }
    let height = next.hitbox_height(input);
    let mut mover = next.collision_box(height);
    let Some(pushes) = world.piston_pushes(mover, next.tick)? else {
        return Ok(None);
    };
    let mut identity = pushes.identity;
    for push in pushes.value {
        let Some(delta) = push.delta(mover) else {
            continue;
        };
        let view = ScaffoldingView::new(world, mover, false)
            .with_powder_snow(next.fall_distance, input.can_stand_on_snow);
        let motion = resolve_motion(&view, mover, delta, next.on_ground, input.no_clip)?;
        identity = identity.merge(&motion.identity)?;
        next.position = motion.position;
        next.retain_collision_box(motion.aabb, height);
        mover = motion.aabb;
    }
    Ok(Some(identity))
}
