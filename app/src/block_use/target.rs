//! Adapts fresh world evidence to the gameplay-owned held placement intention.
use gameplay::{block_use::PickResult, block_use::PlacementTarget};
use semantic_input::identity::{InputAuthorityGeneration, InputSequence};
use sim::CollisionWorld;
use simulation_clock::identity::PositionAuthorityGeneration;
use {
    super::*,
    gameplay::interaction_authority::FrozenBlockObservation,
    gameplay::mining::{creative_reach, survival_reach},
};

/// Recasts the pre-tick frame pick against the current world and resolves the held support.
#[allow(clippy::too_many_arguments)]
pub(super) fn observe_use_target(
    player_runtime: &client_state::PlayerRuntime,
    context: &BlockUseContext,
    origin: &InteractionOriginSnapshot,
    input_mode: PlayerInputMode,
    survival: bool,
    input_authority: (InputAuthorityGeneration, InputSequence),
    position_authority_generation: PositionAuthorityGeneration,
    runtime: &BlockUseRuntime,
    pick: Option<FramePick>,
    trigger: ItemUseTrigger,
    state: &gameplay::movement::UnsentSampleView,
) -> Option<FrozenBlockObservation> {
    let reach = if survival {
        survival_reach(input_mode)
    } else {
        creative_reach(input_mode)
    };
    let stream = context.client_world.stream.as_ref()?;
    let ray = origin.outbound_ray()?;
    let pick = pick.filter(|pick| {
        pick.session_generation == ray.session_generation()
            && pick.actor_session_id == ray.actor_session_id()
    })?;
    let (velocity, sneaking) = (state.delta, state.sneaking);
    let selection = verified_use_selection(player_runtime, &context.ui)?;
    let identifier = stream
        .authority()
        .item_identifier(selection.item.network_id());
    let liquids = gameplay::block_use::uses_liquid_pick(&selection.item, identifier.as_deref());
    let orientation_hit = held_block_store_id(stream, selection.item.block_runtime_id())
        .filter(|block| {
            gameplay::block_use::orientation_sensitive(
                context
                    .collisions
                    .block_facts(stream.network_id_mode(), *block),
            )
        })
        .and_then(|_| runtime.intention.first_world_hit());
    let mut observed = crate::interaction_authority::observe_block_ray_using(
        origin,
        &context.ui,
        &context.client_world,
        &context.collisions,
        selection,
        player_runtime.facts.player_game_mode(),
        liquids,
        (
            input_mode,
            reach,
            input_authority,
            position_authority_generation,
        ),
        |world, _, _, reach| {
            let vector = |value: bevy::prelude::Vec3| {
                sim::Vec3::new(f64::from(value.x), f64::from(value.y), f64::from(value.z))
            };
            let origin = vector(pick.origin);
            let hit = if trigger == ItemUseTrigger::PlayerInput {
                if liquids {
                    pick.hits.liquid
                } else {
                    pick.hits.ordinary
                }
            } else {
                super::pick::refresh(world, pick, liquids, state, reach, stream.authority())?
            };
            let Some(hit) = hit else {
                return Ok(None);
            };
            let block = match hit {
                PickResult::Block(block) => Some(block),
                _ => None,
            };
            let endpoint = hit.point().unwrap_or(pick.hits.endpoint);
            let endpoint = sim::Vec3::new(
                f64::from(endpoint[0]),
                f64::from(endpoint[1]),
                f64::from(endpoint[2]),
            );
            let target = runtime.intention.target(
                block
                    .filter(|hit| !hit.indirect || trigger == ItemUseTrigger::PlayerInput)
                    .map(|hit| PlacementTarget {
                        position: hit.position,
                        face: hit.face,
                    }),
                [origin.x as f32, origin.y as f32, origin.z as f32],
                [endpoint.x as f32, endpoint.y as f32, endpoint.z as f32],
                velocity,
                sneaking,
            );
            let Some(target) = target else {
                return Ok(None);
            };
            let primary = world
                .primary_is_air(target.position)?
                .ok_or(sim::WorldQueryError::InvalidRayOrigin)?;
            let runtime_id = world.primary_runtime_id(target.position)?;
            // Miss continuation uses the support corner; a real pick keeps its click coordinates.
            let click = orientation_hit
                .or_else(|| hit.point())
                .map(|point| point.map(f64::from))
                .unwrap_or_else(|| target.position.map(f64::from));
            Ok(Some(sim::BlockHit {
                block_pos: target.position,
                face: target.face,
                hit_local: sim::Vec3::new(
                    click[0] - target.position[0] as f64,
                    click[1] - target.position[1] as f64,
                    click[2] - target.position[2] as f64,
                ),
                hit_world: endpoint,
                runtime_id,
                distance: (endpoint - origin).length_squared().sqrt(),
                identity: primary.identity,
                overlay: block
                    .filter(|hit| hit.position == target.position)
                    .and_then(|hit| hit.overlay),
            }))
        },
    )
    .ok()?
    .value?;
    observed.ray.origin = pick.origin.to_array();
    observed.ray.direction = pick.direction.to_array();
    Some(observed)
}
