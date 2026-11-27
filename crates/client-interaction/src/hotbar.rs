//! Samples semantic hotbar selection before transport admission.
use std::time::Duration;

use bevy::{
    input::mouse::{AccumulatedMouseScroll, MouseScrollUnit},
    prelude::{Res, ResMut, Resource},
    time::{Real, Time},
};
use client_state::{PlayerRuntime, RuntimeSettings, ScreenFocus, SemanticInputSnapshot};
use protocol::HOTBAR_SLOT_COUNT;
use semantic_input::{Action, HOTBAR_DIGIT_ACTIONS};

/// Raw wheel pixels per hotbar notch.
const PIXELS_PER_NOTCH: f32 = 16.0;
/// Half a notch starts a cycle; smaller trackpad drift never selects a slot.
const NOTCH_THRESHOLD: f32 = 0.5;
/// One cycle per interval keeps wheel inertia from spinning the hotbar.
const CYCLE_COOLDOWN: Duration = Duration::from_millis(100);

/// Converts a backend wheel delta to the pixel scale consumed by [`WheelAccumulator::advance`].
/// One physical line notch is one hotbar notch (16 px); pixel deltas keep their original scale.
pub fn wheel_pixels(delta_y: f32, unit: MouseScrollUnit) -> f32 {
    match unit {
        MouseScrollUnit::Line => delta_y * PIXELS_PER_NOTCH,
        MouseScrollUnit::Pixel => delta_y,
    }
}

/// Retained wheel motion for one gameplay session. Time comes from the caller so tests advance
/// it by hand; only the ECS system reads the frame clock.
#[derive(Debug, Default, Resource)]
pub struct WheelAccumulator {
    notches: f32,
    last_cycle: Option<Duration>,
}

impl WheelAccumulator {
    /// Drops pending motion when gameplay loses the wheel or the setting is turned off.
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Accumulates one frame's raw pixel delta and returns at most one signed slot step.
    /// A positive pixel delta selects the previous slot, matching the old wheel direction.
    pub fn advance(&mut self, delta_y: f32, now: Duration) -> i32 {
        if !delta_y.is_finite() || delta_y == 0.0 {
            self.notches = 0.0;
            return 0;
        }
        let delta = delta_y / PIXELS_PER_NOTCH;
        if self.notches != 0.0 && self.notches.is_sign_negative() != delta.is_sign_negative() {
            self.notches = 0.0;
        }
        self.notches += delta;
        if self.notches.abs() < NOTCH_THRESHOLD {
            return 0;
        }
        if self
            .last_cycle
            .is_some_and(|last| now.saturating_sub(last) < CYCLE_COOLDOWN)
        {
            return 0;
        }
        let direction = if self.notches > 0.0 { -1 } else { 1 };
        self.notches = 0.0;
        self.last_cycle = Some(now);
        direction
    }
}

/// Resolves one selection action using the same slot identities as semantic hotbar input.
pub fn slot_for_action(action: Action, current: u8) -> Option<u8> {
    if let Some(index) = HOTBAR_DIGIT_ACTIONS
        .iter()
        .position(|candidate| *candidate == action)
    {
        return u8::try_from(index).ok();
    }
    let delta = cycle_delta(action)?;
    Some((i32::from(current) + delta).rem_euclid(i32::from(HOTBAR_SLOT_COUNT)) as u8)
}

/// Queues number-key, controller and mouse-wheel selection using the current screen authority.
pub fn queue_hotbar_selection(
    mut player_runtime: ResMut<PlayerRuntime>,
    input: Res<SemanticInputSnapshot>,
    scroll: Res<AccumulatedMouseScroll>,
    focus: Res<ScreenFocus>,
    settings: Res<RuntimeSettings>,
    time: Option<Res<Time<Real>>>,
    mut wheel: ResMut<WheelAccumulator>,
) {
    if input
        .snapshot()
        .is_some_and(|snapshot| snapshot.ordered.is_some())
    {
        return;
    }
    // Direct number-key selection. The router only resolves Hotbar1..9 in the Gameplay context,
    // so digits typed while chat is focused never reach this snapshot.
    let mut requested: Option<u8> = None;
    for &action in HOTBAR_DIGIT_ACTIONS {
        if input.phase(action).pressed {
            requested = slot_for_action(action, 0);
        }
    }

    // Controller actions are router-gated; raw wheel input needs the same screen ownership.
    let mut cycle: i32 = 0;
    if input.phase(Action::HotbarNext).pressed {
        cycle += cycle_delta(Action::HotbarNext).expect("cycle action");
    }
    if input.phase(Action::HotbarPrevious).pressed {
        cycle += cycle_delta(Action::HotbarPrevious).expect("cycle action");
    }
    let wheel_enabled = settings
        .user_settings_update()
        .1
        .gameplay
        .mouse_wheel_hotbar_scrolling;
    if wheel_enabled && !focus.absorbs_input {
        let now = time.map_or(Duration::ZERO, |clock| clock.elapsed());
        cycle += wheel.advance(wheel_pixels(scroll.delta.y, scroll.unit), now);
    } else {
        // A disabled wheel or a screen that owns input must leave no pending motion behind.
        wheel.clear();
    }

    if requested.is_none() && cycle != 0 {
        let current = i32::from(
            player_runtime
                .inventory
                .selected_hotbar_slot(player_runtime.facts.player_game_mode())
                .unwrap_or(0)
                .min(HOTBAR_SLOT_COUNT - 1),
        );
        let slots = i32::from(HOTBAR_SLOT_COUNT);
        requested = Some((((current + cycle) % slots + slots) % slots) as u8);
    }

    if let Some(target) = requested {
        let game_mode = player_runtime.facts.player_game_mode();
        player_runtime
            .inventory
            .queue_local_hotbar_selection(target, game_mode);
    }
}

/// Supplies one direction policy to both frame and ordered selection consumers.
fn cycle_delta(action: Action) -> Option<i32> {
    match action {
        Action::HotbarNext => Some(1),
        Action::HotbarPrevious => Some(-1),
        _ => None,
    }
}

#[cfg(test)]
mod wheel_tests {
    use super::*;
    use bevy::{
        ecs::system::RunSystemOnce,
        input::mouse::{AccumulatedMouseScroll, MouseScrollUnit},
        prelude::{App, Vec2},
    };
    use client_state::{RuntimeSettings, ScreenFocus, SemanticInputSnapshot};
    use semantic_input::{
        ActionPhase, ActionSnapshot, InputMode,
        finalized::FinalizedInput,
        identity::{InputAuthorityGeneration, InputSequence},
    };
    use test_time::ManualClock;

    /// Runs one production hotbar frame and reports the predicted slot and queued command.
    /// Existing pixel-based callers keep their 16 px per notch meaning explicitly.
    fn wheel_frame_with(
        initial_slot: u8,
        delta_y: f32,
        focus: ScreenFocus,
        input: SemanticInputSnapshot,
        wheel_enabled: bool,
    ) -> (Option<u8>, Option<u8>) {
        wheel_frame_with_unit(
            initial_slot,
            delta_y,
            focus,
            input,
            wheel_enabled,
            MouseScrollUnit::Pixel,
        )
    }

    /// Runs one production hotbar frame with the backend wheel unit preserved.
    fn wheel_frame_with_unit(
        initial_slot: u8,
        delta_y: f32,
        focus: ScreenFocus,
        input: SemanticInputSnapshot,
        wheel_enabled: bool,
        unit: MouseScrollUnit,
    ) -> (Option<u8>, Option<u8>) {
        let mut player = PlayerRuntime(player_state::PlayerState::new(1));
        player.inventory.set_local_selected_slot(initial_slot);
        let mut user_settings = ui::UserSettings::default();
        user_settings.gameplay.mouse_wheel_hotbar_scrolling = wheel_enabled;
        let mut runtime = RuntimeSettings::default();
        runtime.replace_user_settings(user_settings);
        let mut app = App::new();
        app.insert_resource(player)
            .insert_resource(input)
            .insert_resource(runtime)
            .insert_resource(focus)
            .insert_resource(WheelAccumulator::default())
            .insert_resource(AccumulatedMouseScroll {
                delta: Vec2::new(0.0, delta_y),
                unit,
                ..Default::default()
            });
        app.world_mut()
            .run_system_once(queue_hotbar_selection)
            .expect("the production hotbar system runs once");
        let player = app.world().resource::<PlayerRuntime>();
        (
            player.selected_hotbar_slot(),
            player.inventory.pending_hotbar_selection(),
        )
    }

    /// Runs one hotbar frame with the default-on wheel setting.
    fn wheel_frame(
        initial_slot: u8,
        delta_y: f32,
        focus: ScreenFocus,
        input: SemanticInputSnapshot,
    ) -> (Option<u8>, Option<u8>) {
        wheel_frame_with(initial_slot, delta_y, focus, input, true)
    }

    /// A finalized frame with no action edge at all.
    fn no_actions() -> SemanticInputSnapshot {
        SemanticInputSnapshot::default()
    }

    /// A finalized frame with exactly one pressed action and no wheel delta.
    fn pressing(action: Action) -> SemanticInputSnapshot {
        let mut phases = [ActionPhase::default(); Action::COUNT];
        phases[action as usize].pressed = true;
        SemanticInputSnapshot(FinalizedInput::from_finalized(ActionSnapshot {
            ordered: None,
            admitted_desktop: Vec::new(),
            frame_sequence: InputSequence::new(1),
            authority_generation: InputAuthorityGeneration::INITIAL,
            movement: [0.0; 2],
            raw_movement: [0.0; 2],
            analogue_movement: [0.0; 2],
            movement_buttons: Default::default(),
            look_delta: [0.0; 2],
            input_mode: InputMode::KeyboardMouse,
            phases,
            release_reasons: [None; Action::COUNT],
            authority_released: false,
        }))
    }

    #[test]
    fn raw_wheel_delta_y_cycles_slot_without_any_semantic_action() {
        assert_eq!(
            wheel_frame(0, -PIXELS_PER_NOTCH, ScreenFocus::default(), no_actions()),
            (Some(1), Some(1))
        );
        assert_eq!(
            wheel_frame(2, PIXELS_PER_NOTCH, ScreenFocus::default(), no_actions()),
            (Some(1), Some(1))
        );
    }

    #[test]
    fn raw_one_line_notch_cycles_once_when_enabled_and_is_silent_when_disabled() {
        assert_eq!(
            wheel_frame_with_unit(
                0,
                -3.0,
                ScreenFocus::default(),
                no_actions(),
                true,
                MouseScrollUnit::Line
            ),
            (Some(1), Some(1)),
            "one physical line notch is one hotbar step"
        );
        assert_eq!(
            wheel_frame_with_unit(
                0,
                -3.0,
                ScreenFocus::default(),
                no_actions(),
                false,
                MouseScrollUnit::Line
            ),
            (Some(0), None),
            "the same line notch is silent when the setting is off"
        );
    }

    #[test]
    fn raw_two_rapid_line_notches_step_at_most_once_per_hundred_ms() {
        let mut player = PlayerRuntime(player_state::PlayerState::new(1));
        player.inventory.set_local_selected_slot(0);
        let mut user_settings = ui::UserSettings::default();
        user_settings.gameplay.mouse_wheel_hotbar_scrolling = true;
        let mut runtime = RuntimeSettings::default();
        runtime.replace_user_settings(user_settings);
        let mut app = App::new();
        app.insert_resource(player)
            .insert_resource(no_actions())
            .insert_resource(runtime)
            .insert_resource(ScreenFocus::default())
            .insert_resource(WheelAccumulator::default())
            .insert_resource(Time::<Real>::default())
            .insert_resource(AccumulatedMouseScroll {
                delta: Vec2::new(0.0, -3.0),
                unit: MouseScrollUnit::Line,
                ..Default::default()
            });
        app.world_mut()
            .run_system_once(queue_hotbar_selection)
            .unwrap();
        assert_eq!(
            app.world()
                .resource::<PlayerRuntime>()
                .inventory
                .pending_hotbar_selection(),
            Some(1)
        );
        app.world_mut()
            .run_system_once(queue_hotbar_selection)
            .unwrap();
        assert_eq!(
            app.world()
                .resource::<PlayerRuntime>()
                .inventory
                .pending_hotbar_selection(),
            Some(1),
            "the rapid second notch must stay inside the cooldown"
        );
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(Duration::from_millis(100));
        app.world_mut()
            .run_system_once(queue_hotbar_selection)
            .unwrap();
        assert_eq!(
            app.world()
                .resource::<PlayerRuntime>()
                .inventory
                .pending_hotbar_selection(),
            Some(2)
        );
    }

    #[test]
    fn raw_pixel_delta_still_divides_by_sixteen() {
        assert_eq!(
            wheel_frame_with_unit(
                0,
                -PIXELS_PER_NOTCH,
                ScreenFocus::default(),
                no_actions(),
                true,
                MouseScrollUnit::Pixel
            ),
            (Some(1), Some(1))
        );
        assert_eq!(
            wheel_frame_with_unit(
                0,
                -7.0,
                ScreenFocus::default(),
                no_actions(),
                true,
                MouseScrollUnit::Pixel
            ),
            (Some(0), None),
            "sub-notch pixel drift remains below the threshold"
        );
    }

    #[test]
    fn absorbed_screen_input_ignores_raw_wheel_delta() {
        let focus = ScreenFocus {
            absorbs_input: true,
            ..ScreenFocus::default()
        };
        assert_eq!(
            wheel_frame(0, -PIXELS_PER_NOTCH, focus, no_actions()),
            (Some(0), None)
        );
    }

    /// Turning the option off leaves no slot change and no queued command for raw wheel input.
    #[test]
    fn opting_out_makes_the_wheel_completely_silent() {
        assert_eq!(
            wheel_frame_with(
                0,
                -PIXELS_PER_NOTCH,
                ScreenFocus::default(),
                no_actions(),
                false
            ),
            (Some(0), None)
        );
        assert_eq!(
            wheel_frame_with(
                0,
                PIXELS_PER_NOTCH,
                ScreenFocus::default(),
                no_actions(),
                false
            ),
            (Some(0), None)
        );
    }

    #[test]
    fn hotbar_next_action_still_cycles_without_wheel_delta() {
        assert_eq!(
            wheel_frame(0, 0.0, ScreenFocus::default(), pressing(Action::HotbarNext)),
            (Some(1), Some(1))
        );
    }

    #[test]
    fn sub_notch_scroll_is_accumulated_before_it_cycles() {
        let clock = ManualClock::new();
        let mut wheel = WheelAccumulator::default();
        assert_eq!(wheel.advance(-7.0, clock.elapsed()), 0);
        assert_eq!(wheel.advance(-7.0, clock.elapsed()), 1);
    }

    #[test]
    fn one_hundred_ms_cooldown_limits_repeated_wheel_steps() {
        let clock = ManualClock::new();
        let mut wheel = WheelAccumulator::default();
        assert_eq!(wheel.advance(-PIXELS_PER_NOTCH, clock.elapsed()), 1);
        clock.advance(Duration::from_millis(99));
        assert_eq!(wheel.advance(-PIXELS_PER_NOTCH, clock.elapsed()), 0);
        clock.advance(Duration::from_millis(1));
        assert_eq!(wheel.advance(-PIXELS_PER_NOTCH, clock.elapsed()), 1);
    }

    #[test]
    fn trackpad_inertia_does_not_spin_multiple_slots_in_one_interval() {
        let clock = ManualClock::new();
        let mut wheel = WheelAccumulator::default();
        let mut steps = 0;
        for _ in 0..20 {
            steps += wheel.advance(-4.0, clock.elapsed()).abs();
            clock.advance(Duration::from_millis(5));
        }
        assert_eq!(steps, 1);
    }

    #[test]
    fn reversing_wheel_direction_discards_pending_motion() {
        let clock = ManualClock::new();
        let mut wheel = WheelAccumulator::default();
        assert_eq!(wheel.advance(-7.0, clock.elapsed()), 0);
        assert_eq!(wheel.advance(PIXELS_PER_NOTCH, clock.elapsed()), -1);
    }

    #[test]
    fn clearing_the_accumulator_discards_pending_motion() {
        let mut wheel = WheelAccumulator::default();
        assert_eq!(wheel.advance(-7.0, Duration::ZERO), 0);
        wheel.clear();
        assert_eq!(wheel.advance(-7.0, Duration::ZERO), 0);
    }
}
