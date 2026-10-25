//! Captures complete frame picks and refreshes their bounded block segments.
use super::*;
use client_interaction::block_use::FrameHits;
use gameplay::{
    block_use::{BlockPick, PickResult},
    melee::{Crosshair, classify_block_hit, pick_actor},
    mining::{creative_reach, survival_reach},
};

/// Converts collision evidence into the geometry retained by a native-style frame pick.
fn block_pick(hit: &sim::BlockHit, indirect: bool) -> BlockPick {
    BlockPick {
        position: hit.block_pos,
        face: hit.face,
        point: [
            hit.hit_world.x as f32,
            hit.hit_world.y as f32,
            hit.hit_world.z as f32,
        ],
        indirect,
        overlay: hit.overlay,
    }
}

/// Widens the already rounded frame coordinates for the collision query boundary.
fn vector(value: [f32; 3]) -> sim::Vec3 {
    sim::Vec3::new(
        f64::from(value[0]),
        f64::from(value[1]),
        f64::from(value[2]),
    )
}

/// Captures both ordinary and liquid-aware results while this frame's world and ray are current.
pub(crate) fn retain_block_use_pick(
    context: BlockUseContext,
    player: Res<client_state::PlayerRuntime>,
    movement: Res<MovementTicker>,
    mut runtime: ResMut<BlockUseRuntime>,
) {
    let Some(ray) = context.origin.outbound_ray() else {
        runtime.clear_picks();
        return;
    };
    let Some(stream) = context.client_world.stream.as_ref() else {
        runtime.clear_picks();
        return;
    };
    if !crate::interaction_authority::ray_is_current(
        ray,
        simulation_clock::identity::SessionGeneration::new(context.ui.session_id()),
        stream,
    ) {
        runtime.clear_picks();
        return;
    }
    let Some(state) = movement.build_action_state() else {
        runtime.unavailable_pick();
        return;
    };
    let mode = protocol_input_mode(
        context
            .input
            .snapshot()
            .map_or(semantic_input::InputMode::KeyboardMouse, |input| {
                input.input_mode
            }),
    );
    let game_mode = player.facts.player_game_mode();
    let reach = if game_mode == Some(PlayerGameMode::Creative) {
        creative_reach(mode)
    } else {
        survival_reach(mode)
    };
    let selection = client_interaction::mining::verified_selection(&player);
    let mut pick_context = crate::interaction_authority::pick_context(
        stream,
        &context.collisions,
        game_mode,
        selection
            .as_ref()
            .map(|selection| selection.item.block_runtime_id()),
    );
    let origin = ray.origin().to_array();
    let direction = ray.direction().to_array();
    let Some(normalized) = sim::normalize_direction(direction) else {
        runtime.unavailable_pick();
        return;
    };
    let endpoint = std::array::from_fn(|axis| origin[axis] + normalized[axis] * reach as f32);
    let actor = pick_actor(
        stream.authority().remote_actors(),
        context.ui.gameplay_hud().mount_unique_id(),
        origin,
        direction,
        reach,
    );
    // Only terrain through the actor's block-occlusion margin can change its classification.
    let query_reach = actor.map_or(reach, |hit| {
        f64::from(hit.distance as f32 + assets::facts::entity::DEFAULT_ACTOR_PICK_RADIUS).min(reach)
    });
    let overlay_ids = stream.authority().decode_ids(stream.current_dimension());
    let mut capture = |liquids| {
        pick_context.liquids = liquids;
        let world = PaletteWorld::new(
            stream.collision_store(),
            context.collisions.registry(stream.network_id_mode()),
            stream.current_dimension(),
            pick_context,
        )
        .with_cell_overlays(&overlay_ids);
        let hit = world
            .block_interaction_ray_prefix_current(
                vector(origin),
                vector(direction),
                reach,
                query_reach,
            )
            .ok()?;
        if let Crosshair::Actor(actor) = classify_block_hit(actor, origin, hit.as_ref(), reach) {
            return Some(
                PickResult::Actor {
                    identity: stream.authority().actor_lifetime(actor.runtime_id)?,
                    point: actor.point,
                }
                .trimmed(state.position, reach),
            );
        }
        if let Some(hit) = hit {
            let block = block_pick(&hit, false);
            return Some(PickResult::Block(block).trimmed(state.position, reach));
        }
        let indirect = world
            .block_use_miss_support_current(vector(origin), vector(direction))
            .ok()?;
        Some(indirect.map_or(PickResult::Miss, |hit| {
            PickResult::Block(block_pick(&hit, true)).trimmed(state.position, reach)
        }))
    };
    let (ordinary, liquid) = (capture(false), capture(true));
    if ordinary.is_some() || liquid.is_some() {
        runtime.retain_pick(
            &context.origin,
            movement.position_authority_identity(),
            FrameHits {
                endpoint,
                ordinary,
                liquid,
            },
        );
    } else {
        runtime.unavailable_pick();
    }
}

/// Reclips a saved observation to its old intercept, retaining eligible original hits on a miss.
pub(super) fn refresh(
    world: &PaletteWorld<'_>,
    pick: FramePick,
    liquids: bool,
    state: &gameplay::movement::UnsentSampleView,
    reach: f64,
    authority: &client_world::WorldAuthority,
) -> Result<Option<PickResult>, sim::WorldQueryError> {
    let original = if liquids {
        pick.hits.liquid
    } else {
        pick.hits.ordinary
    };
    let Some(original) = original else {
        return Ok(None);
    };
    let origin = vector(pick.origin.to_array());
    let mut endpoint = original
        .point()
        .map(vector)
        .unwrap_or(vector(pick.hits.endpoint));
    if matches!(original, PickResult::Actor { .. }) {
        endpoint = vector(pick.hits.endpoint);
    }
    let mut actor_alive = false;
    if let PickResult::Actor { identity, .. } = original
        && let Some(actor) = live_actor(authority, identity)
    {
        actor_alive = true;
        if let Some(point) = actor.bounding_box().and_then(|(min, max)| {
            sim::Aabb::new(vector(min), vector(max)).clip_segment_point(origin, endpoint, false)
        }) {
            endpoint = point;
        }
    }
    let hit = world.block_interaction_segment_current(origin, endpoint)?;
    Ok(Some(original.refreshed(
        hit.as_ref().map(|hit| block_pick(hit, false)),
        state.position,
        reach,
        actor_alive,
    )))
}

/// Resolves the same actor lifetime that supplied a saved hit, rejecting reused runtime IDs.
pub(super) fn live_actor(
    authority: &client_world::WorldAuthority,
    identity: actor_animation::ActorLifetimeId,
) -> Option<&client_world::ActorSnapshot> {
    (authority.actor_lifetime(identity.runtime_id) == Some(identity))
        .then(|| authority.actor(identity.runtime_id))
        .flatten()
}
