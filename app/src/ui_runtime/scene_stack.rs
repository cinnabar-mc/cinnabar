//! Ordered app adapter for UI scene retirement.
use crate::runtime::world::ClientWorld;
use bevy::prelude::{Local, Real, Res, ResMut, Time};
use client_interaction::block_use::BlockUseRuntime;
use client_interaction::movement::LocalPhysicsController;
use client_shell::ui_resources::UiRuntimeResource;
use client_ui::ui_runtime::presentation::forms::scene_policy::MenuScene;
use {client_shell::menu::MenuRuntime, client_state::PlayerRuntime};

/// Applies damage-driven scene retirement at its existing authority phase.
pub(crate) fn close_scenes_on_player_hurt(
    mut player_runtime: ResMut<PlayerRuntime>,
    menu: Option<Res<MenuRuntime>>,
    mut runtime: ResMut<UiRuntimeResource>,
    world: Option<Res<ClientWorld>>,
    physics: Option<Res<LocalPhysicsController>>,
    block_use: Option<Res<BlockUseRuntime>>,
    time: Option<Res<Time<Real>>>,
    mut anchor: Local<Option<(u64, u64, u64)>>,
) {
    let session = runtime.session_id();
    let generation = player_runtime.inventory.ledger().storage_generation();
    if let (Some(generation), Some(block_use), Some(time)) =
        (generation, block_use.as_deref(), time.as_deref())
    {
        if anchor.is_none_or(|(saved_session, saved_generation, _)| {
            saved_session != session || saved_generation != generation
        }) {
            *anchor = block_use
                .recent_actor_use(session, time.elapsed())
                .map(|actor| (session, generation, actor));
        }
    } else if generation.is_none() {
        *anchor = None;
    }
    if let (Some(stream), Some(state), Some(actor_id)) = (
        world.as_deref().and_then(|world| world.stream.as_ref()),
        physics.as_deref().and_then(|physics| physics.state()),
        player_runtime.inventory.ledger().window_actor(),
    ) {
        let authority = stream.authority();
        let actor_position = anchor
            .as_ref()
            .and_then(|(_, _, actor)| authority.actor(*actor))
            .or_else(|| authority.inventory_ui().container_actor(actor_id))
            .map(|actor| actor.position);
        runtime.close_distant_entity_storage(
            &mut player_runtime,
            [state.position.x, state.position.y, state.position.z],
            actor_position,
        );
    }
    runtime.close_scenes_on_player_hurt(
        &mut player_runtime,
        menu.as_deref().map(|menu| menu as &dyn MenuScene),
    );
}
