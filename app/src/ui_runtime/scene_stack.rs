//! Ordered app adapter for UI scene retirement.
use crate::runtime::world::ClientWorld;
use bevy::{
    ecs::system::SystemParam,
    prelude::{Local, Real, Res, ResMut, Time},
};
use client_interaction::block_use::BlockUseRuntime;
use client_interaction::movement::LocalPhysicsController;
use client_shell::ui_resources::UiRuntimeResource;
use client_ui::ui_runtime::presentation::forms::scene_policy::MenuScene;
use {client_shell::menu::MenuRuntime, client_state::PlayerRuntime};

/// World and interaction facts used to retire distant entity storage.
#[derive(SystemParam)]
pub(crate) struct EntityStorageInputs<'w> {
    world: Option<Res<'w, ClientWorld>>,
    physics: Option<Res<'w, LocalPhysicsController>>,
    block_use: Option<Res<'w, BlockUseRuntime>>,
    time: Option<Res<'w, Time<Real>>>,
}

/// Applies damage-driven scene retirement at its existing authority phase.
pub(crate) fn close_scenes_on_player_hurt(
    mut player_runtime: ResMut<PlayerRuntime>,
    menu: Option<Res<MenuRuntime>>,
    mut runtime: ResMut<UiRuntimeResource>,
    storage: EntityStorageInputs,
    mut anchor: Local<Option<(u64, u64, u64)>>,
) {
    let EntityStorageInputs {
        world,
        physics,
        block_use,
        time,
    } = storage;
    let session = runtime.session_id();
    let generation = player_runtime.inventory.ledger().storage_generation();
    let stream = world.as_deref().and_then(|world| world.stream.as_ref());
    let window_actor = stream.and_then(|stream| {
        stream
            .authority()
            .inventory_ui()
            .container_actor(player_runtime.inventory.ledger().window_actor()?)
            .filter(|actor| actor.runtime_id != stream.local_player_runtime_id())
    });
    if let (Some(generation), Some(block_use), Some(time)) =
        (generation, block_use.as_deref(), time.as_deref())
    {
        if window_actor.is_some()
            || anchor.is_none_or(|(saved_session, saved_generation, _)| {
                saved_session != session || saved_generation != generation
            })
        {
            *anchor = window_actor
                .map(|actor| actor.runtime_id)
                .or_else(|| block_use.recent_actor_use(session, time.elapsed()))
                .map(|actor| (session, generation, actor));
        }
    } else if generation.is_none() {
        *anchor = None;
    }
    if let (Some(stream), Some(state), Some(_)) = (
        stream,
        physics.as_deref().and_then(|physics| physics.state()),
        player_runtime.inventory.ledger().window_actor(),
    ) {
        let authority = stream.authority();
        let actor_position = window_actor
            .or_else(|| {
                anchor
                    .as_ref()
                    .and_then(|(_, _, actor)| authority.actor(*actor))
            })
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
