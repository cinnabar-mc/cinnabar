//! Retained block-use picks and placement observations.
use crate::{mining::verified_selection, movement::PhysicsCollisionRegistries};
use bevy::prelude::Resource;
use client_presentation::local_player::{InteractionOriginSnapshot, LocalViewPose};
use client_ui::ui_runtime::UiRuntime;
use gameplay::{
    block_use::UseSurroundings, interaction_authority::FrozenBlockObservation,
    mining::FrozenMiningSelection,
};
use sim::PaletteWorld;
use simulation_clock::identity::{PositionAuthorityGeneration, SessionGeneration};

mod prediction_frames;

/// Bevy resource adapter for the gameplay block_use owner.
#[derive(Resource, Debug, Default)]
pub struct BlockUseRuntime {
    owner: gameplay::block_use::BlockUseRuntime,
    previous_pick: Option<FramePick>,
    pending_picks: Vec<FramePick>,
    pub prediction_frames: prediction_frames::PredictionFrames,
}
impl std::ops::Deref for BlockUseRuntime {
    type Target = gameplay::block_use::BlockUseRuntime;
    /// Borrows the gameplay owner at the existing ordered system boundary.
    fn deref(&self) -> &Self::Target {
        &self.owner
    }
}
impl std::ops::DerefMut for BlockUseRuntime {
    /// Mutates the gameplay owner without duplicating its state.
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.owner
    }
}

/// One frame's eye ray; vanilla builds from the picks of the frames before each tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FramePick {
    /// Position authority when the pick was taken; any correction or reanchor retires it.
    authority: (SessionGeneration, PositionAuthorityGeneration),
    pub session_generation: SessionGeneration,
    pub actor_session_id: u64,
    pub origin: bevy::prelude::Vec3,
    pub direction: bevy::prelude::Vec3,
    pub hits: FrameHits,
}

/// Independent frame picks; None means that query was unavailable, not a clear miss.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameHits {
    /// Full float segment endpoint captured before choosing or trimming either hit.
    pub endpoint: [f32; 3],
    pub ordinary: Option<gameplay::block_use::PickResult>,
    pub liquid: Option<gameplay::block_use::PickResult>,
}

impl BlockUseRuntime {
    /// Keeps this frame's published pick for the build actions before the next tick.
    pub fn retain_pick(
        &mut self,
        origin: &InteractionOriginSnapshot,
        authority: (SessionGeneration, PositionAuthorityGeneration),
        hits: FrameHits,
    ) {
        if self
            .pending_picks
            .last()
            .is_some_and(|pick| pick.authority != authority)
        {
            self.pending_picks.clear();
        }
        self.previous_pick = origin.outbound_ray().map(|ray| FramePick {
            authority,
            session_generation: ray.session_generation(),
            actor_session_id: ray.actor_session_id(),
            origin: ray.origin(),
            direction: ray.direction(),
            hits,
        });
        if let Some(pick) = self.previous_pick {
            self.pending_picks.push(pick);
        }
    }

    /// Consumes the accumulated frame picks once, retiring observations from older authority.
    pub fn take_picks(&mut self, movement: &gameplay::movement::MovementTicker) -> Vec<FramePick> {
        let authority = movement.position_authority_identity();
        std::mem::take(&mut self.pending_picks)
            .into_iter()
            .filter(|pick| pick.authority == authority)
            .collect()
    }

    /// Retires only the tick's buffered observations, preserving the latest pick for presses.
    pub fn discard_pending_picks(&mut self) {
        self.pending_picks.clear();
    }

    /// Marks a failed frame query without discarding the earlier observations awaiting a tick.
    pub fn unavailable_pick(&mut self) {
        self.previous_pick = None;
    }

    /// Drops observations when a frame has no usable interaction origin or world.
    pub fn clear_picks(&mut self) {
        self.previous_pick = None;
        self.pending_picks.clear();
    }

    /// The retained pick, unless any reanchor (a replayed correction included) moved the
    /// player since it was taken.
    pub fn pick(&self, movement: &gameplay::movement::MovementTicker) -> Option<FramePick> {
        self.previous_pick
            .filter(|previous| previous.authority == movement.position_authority_identity())
    }
}

/// Freezes player backing unless its local mutation or hotbar change still awaits transport.
pub fn verified_use_selection(
    player_runtime: &client_state::PlayerRuntime,
    ui: &UiRuntime,
) -> Option<FrozenMiningSelection> {
    let ledger = ui.inventory_ledger(player_runtime);
    let selection = verified_selection(player_runtime)?;
    if ledger.slot_backing_transport_pending(selection.slot)
        || player_runtime
            .inventory
            .pending_hotbar_selection()
            .is_some()
    {
        return None;
    }
    Some(selection)
}

#[cfg(test)]
mod pick_tests;
#[cfg(test)]
mod tests;

/// Resolves switch prediction through the caller's observed palette view.
pub fn predicted_toggle(
    collisions: &PhysicsCollisionRegistries,
    stream: &chunk_pipeline::WorldStream,
    world: &PaletteWorld<'_>,
    position: [i32; 3],
    clicked: u32,
) -> Option<([i32; 3], u32)> {
    gameplay::block_use::predicted_toggle(
        collisions,
        stream.network_id_mode(),
        world,
        position,
        clicked,
    )
}
/// Resolves placement through the caller's palette view, preserving its query observation.
pub fn predicted_placement(
    collisions: &PhysicsCollisionRegistries,
    stream: &chunk_pipeline::WorldStream,
    world: &PaletteWorld<'_>,
    item_block: i32,
    observed: &FrozenBlockObservation,
    surroundings: &UseSurroundings,
    view: LocalViewPose,
) -> Option<gameplay::block_use::PredictedPlacement> {
    let mode = stream.network_id_mode();
    let range = stream
        .authority()
        .dimension_range(stream.current_dimension())?;
    let (yaw, pitch, _) = view.rotation().to_euler(bevy::math::EulerRot::YXZ);
    gameplay::block_use::predicted_placement(
        collisions,
        mode,
        held_block_store_id(stream, item_block)?,
        world,
        &gameplay::block_use::PlacementContext {
            clicked: observed.target.position,
            input: gameplay::placement_state::PlacementInput {
                face: observed.target.face,
                click_position: observed.target.relative_hit,
                yaw: gameplay::movement::wire_yaw(180.0 - yaw.to_degrees()),
                pitch: -pitch.to_degrees(),
            },
            surroundings,
            build_height: range.base_sub_chunk_y * 16
                ..(range.base_sub_chunk_y + range.sub_chunk_count as i32) * 16,
        },
    )
}
/// Resolves an item block through the production gameplay boundary.
pub fn held_block_store_id(stream: &chunk_pipeline::WorldStream, item_block: i32) -> Option<u32> {
    gameplay::block_use::held_block_store_id(
        &crate::movement::world_view::GameplayWorldView(stream),
        item_block,
    )
}
