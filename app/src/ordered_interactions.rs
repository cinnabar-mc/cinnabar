//! Dispatches each ordered input edge once in its own frame.

use bevy::ecs::system::{
    CachedSystemId, RegisteredSystem, RegisteredSystemError, SystemId, SystemInput,
};
use bevy::prelude::*;
use client_interaction::{
    block_use::BlockUseRuntime, item_use::ItemUseRuntime, melee::MeleeRuntime,
    movement::MovementTicker,
};
use client_presentation::local_player::{InteractionOriginSnapshot, LocalViewPose};
use client_runtime::network::NetworkHandle;
use client_state::SemanticInputSnapshot;
use gameplay::ordered_interactions::{InteractionScope, Invalidation, OrderedInteractions};
use semantic_input::{Action, ActionEvent, ActionSnapshot};

use crate::runtime::network::resource_packs::PackApplication;

/// The resolved edge and the frame geometry used during its immediate dispatch.
#[derive(Clone)]
pub(crate) struct InteractionInput {
    pub edge: ActionEvent,
    pub context: CapturedContext,
}

/// Geometry, input authority and assistance captured at one dispatch boundary.
#[derive(Clone)]
pub(crate) struct CapturedContext {
    pub attack: crate::melee::crosshair::CapturedInputs,
    pub mode: semantic_input::InputMode,
    pub identity: (
        semantic_input::identity::InputAuthorityGeneration,
        semantic_input::identity::InputSequence,
    ),
    pub origin: InteractionOriginSnapshot,
    pub view: LocalViewPose,
    pub aim: Option<Quat>,
}

/// Tracks consumed frame edges and overload recovery without retaining action attempts.
#[derive(Resource, Default)]
pub(crate) struct InteractionRuntime {
    frames: OrderedInteractions,
    ingested: usize,
    barrier_pending: bool,
    overflowed_batches: u64,
}

impl InteractionRuntime {
    /// Preserves an accepted action when ordered input takes over an unordered frame.
    fn with_existing_build_action(world: &World) -> Self {
        let mut runtime = Self::default();
        let action = if world
            .get_resource::<client_interaction::survival_mining::SurvivalMiningRuntime>()
            .is_some_and(|mining| mining.destroying_target().is_some())
        {
            Some(Action::Attack)
        } else if world
            .resource::<BlockUseRuntime>()
            .last_success_destination()
            .is_some()
        {
            Some(Action::Use)
        } else {
            None
        };
        if let Some(action) = action {
            runtime.frames.observe_build_button(action, true);
        }
        runtime
    }

    /// Whether the next drain will invalidate world-owned work for this scope.
    pub(crate) fn changes_world(
        &self,
        session: simulation_clock::identity::SessionGeneration,
        dimension: i32,
    ) -> bool {
        self.frames.changes_world(session, dimension)
    }
}

/// Picks and later edges wait for camera publication and movement admission.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum DispatchPhase {
    BeforeMovement,
    AfterMovement,
}

impl InteractionRuntime {
    /// Keeps frame-final input behind unresolved edges and older gameplay announcements.
    pub(crate) fn maintenance_pending(&self, input: &SemanticInputSnapshot) -> bool {
        self.frames.quarantined() || self.barrier_pending || self.has_unconsumed_edges(input)
    }

    /// Reports whether any edge still awaits dispatch in this frame.
    pub(crate) fn has_unconsumed_edges(&self, input: &SemanticInputSnapshot) -> bool {
        input
            .snapshot()
            .and_then(|input| input.ordered.as_ref())
            .is_some_and(|ordered| self.ingested < ordered.events().len())
    }
}

/// Reports whether an incomplete physical sample still suppresses held input.
pub(crate) fn is_blocked(runtime: Option<&InteractionRuntime>) -> bool {
    runtime.is_some_and(|runtime| runtime.frames.quarantined())
}

/// Keeps held placement behind accepted work and edges waiting for camera publication.
pub(crate) fn pending(world: &World) -> bool {
    let Some(runtime) = world.get_resource::<InteractionRuntime>() else {
        return false;
    };
    runtime.maintenance_pending(world.resource::<SemanticInputSnapshot>())
        || accepted_work_pending(world)
}

/// Keeps new actions behind an accepted release before and after movement owns its carrier.
pub(crate) fn release_pending(item_use: &ItemUseRuntime, movement: &MovementTicker) -> bool {
    item_use.has_pending_release() || movement.has_held_release()
}

/// Reads the gameplay owners shared by pressed actions and held placement.
fn accepted_work_pending(world: &World) -> bool {
    maintenance_carrier_pending(world)
        || release_pending(
            world.resource::<ItemUseRuntime>(),
            world.resource::<MovementTicker>(),
        )
}

/// Keeps new input behind older mining, block stops and terminal controls.
fn maintenance_carrier_pending(world: &World) -> bool {
    world
        .resource::<MeleeRuntime>()
        .has_queued_work(world.resource::<MovementTicker>())
        || world.resource::<BlockUseRuntime>().stopping()
        || world
            .resource::<NetworkHandle<PackApplication>>()
            .closed_command_has_pending_control()
        || world
            .resource::<crate::runtime::shutdown::FatalError>()
            .0
            .is_some()
}

/// The primary-window query each dispatch reads focus through, kept by its producer system so
/// it is not rebuilt every frame.
pub(crate) type PrimaryWindows = QueryState<&'static Window, With<bevy::window::PrimaryWindow>>;

/// Runs `system` exactly as [`World::run_system_cached_with`] does, but reuses an existing
/// cached registration without moving its id resource out of the world and back.
pub(crate) fn run_cached<I, O, M, S>(
    world: &mut World,
    system: S,
    input: I::Inner<'_>,
) -> Result<O, RegisteredSystemError<I, O>>
where
    I: SystemInput + 'static,
    O: 'static,
    S: IntoSystem<I, O, M> + 'static,
{
    let registered = world
        .get_resource::<CachedSystemId<S>>()
        .map(|id| id.entity)
        .filter(|&entity| {
            world
                .get_entity(entity)
                .is_ok_and(|entity| entity.contains::<RegisteredSystem<I, O>>())
        });
    match registered {
        Some(entity) => world.run_system_with(SystemId::from_entity(entity), input),
        None => world.run_system_cached_with(system, input),
    }
}

/// Reads held state at the consumed cursor, suppressing incomplete physical samples.
pub(crate) fn held(
    runtime: Option<&InteractionRuntime>,
    input: &SemanticInputSnapshot,
    action: Action,
) -> bool {
    if is_blocked(runtime) {
        return false;
    }
    let ordered = runtime.zip(input.snapshot().and_then(|input| input.ordered.as_ref()));
    ordered.map_or_else(
        || input.phase(action).held,
        |(runtime, ordered)| ordered.buttons_at(runtime.ingested).held(action),
    )
}

/// Reads held mining or placement from the latest hand-action intention.
/// Item charging continues to read its own use-button state through `held`.
pub(crate) fn build_held(
    runtime: Option<&InteractionRuntime>,
    input: &SemanticInputSnapshot,
    action: Action,
) -> bool {
    held(runtime, input, action)
        && runtime
            .zip(input.snapshot().and_then(|input| input.ordered.as_ref()))
            .is_none_or(|(runtime, _)| runtime.frames.build_action_active(action))
}

/// Reads unconsumed action events in their original order, unless the batch is quarantined.
pub(crate) fn later_button_events<'a>(
    runtime: Option<&InteractionRuntime>,
    input: &'a SemanticInputSnapshot,
    action: Action,
) -> impl Iterator<Item = &'a ActionEvent> + 'a {
    let consumed = runtime.map_or(0, |runtime| runtime.ingested);
    let blocked = is_blocked(runtime);
    input
        .snapshot()
        .and_then(|input| input.ordered.as_ref())
        .into_iter()
        .flat_map(move |ordered| ordered.events().iter().skip(consumed))
        .filter(move |event| !blocked && event.action == action)
}

/// Cancels unadmitted gestures without deleting gameplay-owned accepted carriers.
fn cancel_pending_owners(world: &mut World) {
    world.resource_mut::<MeleeRuntime>().cancel();
    world.resource_mut::<BlockUseRuntime>().clear_press();
    world
        .resource_mut::<ItemUseRuntime>()
        .cancel_pending_input();
}

/// Captures the geometry published at this action's dispatch boundary.
fn capture(world: &mut World, input: &ActionSnapshot, phase: DispatchPhase) -> CapturedContext {
    let position = world
        .resource::<MovementTicker>()
        .position_authority_identity()
        .1;
    if !world
        .resource::<crate::melee::MeleeTarget>()
        .has_published_inputs(position)
    {
        run_cached(world, crate::melee::prepare_melee, ()).expect("ordered pick preparation");
    }
    let aim = world
        .get_resource::<client_presentation::aim_assist::AimAssistFrame>()
        .zip(world.get_resource::<client_presentation::camera::ServerCameraView>())
        .and_then(|(aim, camera)| {
            client_interaction::camera::aim_assist::action_rotation(aim, camera)
        });
    CapturedContext {
        attack: run_cached(world, crate::melee::ordered::capture, phase)
            .expect("ordered actor geometry"),
        mode: input.input_mode,
        identity: (input.authority_generation, input.frame_sequence),
        origin: world.resource::<InteractionOriginSnapshot>().clone(),
        view: *world.resource::<LocalViewPose>(),
        aim,
    }
}

/// Consumes each frame's edges once; unavailable actions never become later-frame input.
pub(crate) fn drain(world: &mut World, phase: DispatchPhase, windows: &mut PrimaryWindows) {
    let snapshot = world
        .resource::<SemanticInputSnapshot>()
        .snapshot()
        .cloned();
    let Some(snapshot) = snapshot.filter(|input| input.ordered.is_some()) else {
        world
            .resource_mut::<ItemUseRuntime>()
            .finish_ordered_use_buttons();
        run_cached(world, crate::item_use::ordered::synchronize_scope, ())
            .expect("item-use scope synchronization");
        run_cached(
            world,
            crate::item_use::ordered::synchronize_server_events,
            (),
        )
        .expect("item-use server events");
        if world.remove_resource::<InteractionRuntime>().is_some() {
            cancel_pending_owners(world);
        }
        return;
    };
    let ordered = snapshot.ordered.as_ref().expect("ordered snapshot");
    let mut runtime = world
        .remove_resource::<InteractionRuntime>()
        .unwrap_or_else(|| InteractionRuntime::with_existing_build_action(world));
    let authority = world
        .resource::<MovementTicker>()
        .interaction_authority_identity();
    let scope = InteractionScope {
        session: authority.0,
        position: authority.1,
        dimension: world
            .resource::<crate::runtime::world::ClientWorld>()
            .stream
            .as_ref()
            .map_or(0, |stream| stream.current_dimension()),
        input: snapshot.authority_generation,
    };
    let new_frame = runtime.frames.observe_frame(scope, snapshot.frame_sequence);
    if new_frame {
        world
            .resource_mut::<ItemUseRuntime>()
            .finish_ordered_use_buttons();
        run_cached(world, crate::item_use::ordered::synchronize_scope, ())
            .expect("item-use scope synchronization");
        runtime.ingested = 0;
    }
    let focused = windows.single(world).is_ok_and(|window| window.focused)
        && !world.resource::<client_state::ScreenFocus>().menu_visible
        && !world.resource::<client_state::ScreenFocus>().ui_focused
        && !world.resource::<client_state::ScreenFocus>().absorbs_input;
    let active = focused
        && world
            .resource::<client_state::PlayerRuntime>()
            .facts
            .player_game_mode()
            != Some(protocol::PlayerGameMode::Spectator)
        && !world
            .resource::<crate::runtime::world::ClientWorld>()
            .respawn
            .input_held();
    let invalidation = runtime
        .frames
        .synchronize(scope, (new_frame && snapshot.authority_released) || !active);
    if invalidation != Invalidation::None {
        cancel_pending_owners(world);
        if matches!(invalidation, Invalidation::World | Invalidation::Position) {
            world.resource_mut::<MeleeRuntime>().synchronize(authority);
            world
                .resource_mut::<BlockUseRuntime>()
                .synchronize(authority);
            world
                .resource_mut::<client_interaction::survival_mining::SurvivalMiningRuntime>()
                .synchronize(authority);
        }
        if invalidation == Invalidation::World {
            world.resource_mut::<ItemUseRuntime>().cancel_for_respawn();
        }
    }
    run_cached(
        world,
        crate::item_use::ordered::synchronize_server_events,
        (),
    )
    .expect("ordered item-use server events");
    let start = runtime.ingested;
    let tail = ordered.events().get(start..).unwrap_or_default();
    if ordered.overflowed() {
        if new_frame {
            runtime.overflowed_batches = runtime.overflowed_batches.saturating_add(1);
            bevy::log::warn!(
                batches = runtime.overflowed_batches,
                "ordered action burst lost input; waiting for a neutral sample"
            );
        }
        runtime.frames.quarantine();
        cancel_pending_owners(world);
    } else {
        runtime.frames.observe_neutral(ordered.neutral());
    }
    run_cached(world, crate::item_use::ordered::flush_release, ())
        .expect("ordered release carrier");
    let stop_ready = run_cached(
        world,
        crate::block_use::ordered::fence,
        !active || snapshot.authority_released || runtime.frames.quarantined(),
    )
    .expect("accepted block stop carrier");
    if active
        && !snapshot.authority_released
        && !runtime.frames.quarantined()
        && stop_ready
        && !tail.is_empty()
    {
        let mut context = None;
        for edge in tail.iter().copied() {
            if accepted_work_pending(world)
                || !world
                    .resource::<MovementTicker>()
                    .accepts_block_interactions()
            {
                break;
            }
            if phase == DispatchPhase::BeforeMovement
                && edge.action == Action::PickItem
                && edge.pressed
            {
                break;
            }
            if announces_selection(edge)
                && !run_cached(world, crate::hotbar::ordered::flush_existing, ())
                    .expect("older selection carrier")
            {
                break;
            }
            let context = context.get_or_insert_with(|| capture(world, &snapshot, phase));
            runtime
                .frames
                .observe_build_button(edge.action, edge.pressed);
            dispatch(
                world,
                InteractionInput {
                    edge,
                    context: context.clone(),
                },
            );
            runtime.ingested += 1;
        }
    }
    if phase == DispatchPhase::AfterMovement
        || !active
        || snapshot.authority_released
        || runtime.frames.quarantined()
    {
        // A blocked suffix expires only at its final opportunity, without replay next frame.
        if active && !snapshot.authority_released && !runtime.frames.quarantined() {
            for edge in ordered.events().iter().skip(runtime.ingested) {
                runtime
                    .frames
                    .observe_build_button(edge.action, edge.pressed);
            }
        }
        runtime.ingested = ordered.events().len();
        world
            .resource_mut::<ItemUseRuntime>()
            .finish_ordered_use_buttons();
    }
    // Held hand actions keep capturing the selected stack, so they still announce it now.
    let selection_ready = ![Action::Attack, Action::Use]
        .into_iter()
        .any(|action| snapshot.phases[action as usize].held)
        || run_cached(world, crate::hotbar::ordered::flush_existing, ())
            .expect("older selection carrier");
    runtime.barrier_pending = !stop_ready || !selection_ready || maintenance_carrier_pending(world);
    world.insert_resource(runtime);
}

/// Hand edges capture the selected stack, so a pending selection must reach the server first.
/// Selection-only frames leave the announcement to the next tick.
fn announces_selection(edge: ActionEvent) -> bool {
    edge.action == Action::Use
        || (edge.pressed && matches!(edge.action, Action::Attack | Action::PickItem))
}

/// Dispatches one edge without rerunning a frame producer or retaining a failed attempt.
fn dispatch(world: &mut World, input: InteractionInput) {
    let edge = input.edge;
    if edge.action != Action::Use
        && !run_cached(
            world,
            crate::block_use::ordered::fence,
            edge.action == Action::Attack && edge.pressed,
        )
        .expect("older block stop carrier")
    {
        return;
    }
    match edge.action {
        Action::Attack if edge.pressed => {
            run_cached(world, crate::melee::ordered::press, input)
                .expect("ordered attack admission");
        }
        Action::Use => {
            run_cached(
                world,
                crate::item_use::ordered::observe_button,
                input.clone(),
            )
            .expect("ordered item button observation");
            if run_cached(world, crate::block_use::ordered::press, input.clone())
                .expect("ordered block admission")
            {
                run_cached(world, crate::item_use::ordered::press, input)
                    .expect("ordered item admission");
            }
        }
        Action::PickItem if edge.pressed => {
            run_cached(world, crate::pick_block::ordered_pick, input)
                .expect("ordered pick admission");
        }
        action
            if edge.pressed
                && client_interaction::hotbar::slot_for_action(action, 0).is_some()
                && world
                    .resource::<MovementTicker>()
                    .between_ticks_sample()
                    .is_some() =>
        {
            run_cached(world, crate::hotbar::ordered::select, action)
                .expect("ordered selection admission");
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests;
