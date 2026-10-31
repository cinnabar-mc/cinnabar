//! Processes the frame picks accumulated before the next simulation tick.
use super::*;

/// Evaluates buffered picks after all immediately eligible ordered edges have drained.
pub(super) fn maintain_block_use(
    mut player_runtime: bevy::prelude::ResMut<client_state::PlayerRuntime>,
    mut context: BlockUseContext,
    mut runtime: ResMut<BlockUseRuntime>,
    mut swings: ResMut<SwingTracker>,
    mut item_use: ResMut<client_interaction::item_use::ItemUseRuntime>,
    movement: Res<MovementTicker>,
    physics: Option<Res<client_interaction::movement::LocalPhysicsController>>,
) {
    let pick = runtime.pick(&movement);
    let simulates_tick = physics.as_deref().is_some_and(|physics| {
        gameplay::movement::frame_simulates_tick(physics, &movement, context.time.delta())
    });
    // The native hit buffer is cleared by the tick, including ticks without a due build.
    let picks = if simulates_tick {
        runtime.take_picks(&movement)
    } else {
        Vec::new()
    };
    if context.input.phase(Action::Use).pressed
        && !context
            .input
            .snapshot()
            .is_some_and(|input| input.ordered.is_some())
    {
        runtime.forget_press_resolution();
    }
    swings.sync_ticks_for_item(
        movement.interaction_authority_identity(),
        movement.completed_tick(),
        &context.effects,
        crate::melee::selected_attack_timing(&player_runtime, &context.client_world)
            .and_then(|timing| timing.swing_duration_ticks),
    );

    runtime.synchronize(movement.interaction_authority_identity());
    let focused =
        !context.focus.menu_visible && context.windows.single().is_ok_and(|window| window.focused);
    let game_mode = player_runtime.facts.player_game_mode();
    let caps = player_runtime.facts.game_mode_capabilities();
    let Some((input, _)) = context
        .input
        .snapshot()
        .zip(caps)
        .filter(|(_, caps)| focused && !context.focus.ui_focused && caps.can_use_blocks())
    else {
        runtime.clear_press();
        stop_block_use(&mut runtime, &context, false);
        return;
    };
    let input_mode = protocol_input_mode(input.input_mode);
    let input_authority = (input.authority_generation, input.frame_sequence);
    let has_ordered_input = input.ordered.is_some();
    let mut use_phase = context.input.phase(Action::Use);
    if has_ordered_input {
        use_phase.held = crate::ordered_interactions::build_held(
            context.ordered.as_deref(),
            &context.input,
            Action::Use,
        );
        use_phase.pressed = false;
        use_phase.released = false;
    }
    let attacking = crate::ordered_interactions::build_held(
        context.ordered.as_deref(),
        &context.input,
        Action::Attack,
    );
    if attacking {
        runtime.clear_press();
    }
    if (runtime.stopping()
        || (runtime.last_success_destination().is_some()
            && (attacking || use_phase.pressed || !use_phase.held)))
        && !stop_block_use(&mut runtime, &context, use_phase.pressed && !attacking)
    {
        return;
    }
    if !runtime.observe_use(
        use_phase.held,
        use_phase.pressed,
        attacking,
        movement.accepts_block_interactions(),
    ) {
        return;
    }
    // An attack in the same frame is handled first: the use waits until melee resolves it,
    // and an actor hit then holds the use off for the post-attack window.
    if (!has_ordered_input && context.input.phase(Action::Attack).pressed)
        || context.melee.press_pending()
    {
        return;
    }
    let Some(selection) = verified_use_selection(&player_runtime, &context.ui) else {
        // Unconfirmed inventory suspends attempts while preserving the held action.
        return;
    };
    runtime.selection_changed(&selection);
    // Vanilla builds before each simulation tick, from the last completed tick's end state;
    // this runs before the frame's physics, so a placed block is in the world the tick sees.
    let Some(state) = movement.build_action_state() else {
        return;
    };
    let tick = state.tick.saturating_add(1);
    let clock = RepeatClock::for_state(context.time.elapsed(), &state, game_mode);
    if context
        .melee
        .blocks_use_at(u64::try_from(clock.now.as_millis()).unwrap_or(u64::MAX))
    {
        runtime.clear_press();
        return;
    }
    let picks = if runtime.press_pending() {
        pick.into_iter().collect::<Vec<_>>()
    } else {
        picks
    };
    for pick in picks {
        let Some((trigger, due)) = runtime.due(use_phase.held, tick, clock) else {
            break;
        };
        if trigger == ItemUseTrigger::SimulationTick && !simulates_tick {
            break;
        }
        let Some(selection) = verified_use_selection(&player_runtime, &context.ui) else {
            break;
        };
        if actor_use::produce(
            &context,
            &mut runtime,
            &selection,
            &player_runtime,
            pick,
            &state,
            input_mode,
            trigger,
            due,
            clock,
        ) {
            continue;
        }
        let Some(observed) = observe_use_target(
            &player_runtime,
            &context,
            &context.origin,
            input_mode,
            clock.survival,
            input_authority,
            movement.position_authority_identity().1,
            &runtime,
            Some(pick),
            trigger,
            &state,
        ) else {
            runtime.record(trigger, due, tick, LocalUse::Nothing, clock);
            continue;
        };
        let mut attempt =
            build::BuildInput::new(observed, state, trigger, due, clock, *context.view);
        build::advance(
            &mut player_runtime,
            &mut context,
            &mut runtime,
            &mut item_use,
            &mut swings,
            &movement,
            &mut attempt,
        );
    }
}

/// A tick still retires its observations while ordered input suspends held-use dispatch.
pub(super) fn discard_tick_picks(
    time: Res<Time<Real>>,
    movement: Res<MovementTicker>,
    physics: Option<Res<client_interaction::movement::LocalPhysicsController>>,
    mut runtime: ResMut<BlockUseRuntime>,
) {
    if physics.as_deref().is_some_and(|physics| {
        gameplay::movement::frame_simulates_tick(physics, &movement, time.delta())
    }) {
        runtime.discard_pending_picks();
    }
}
