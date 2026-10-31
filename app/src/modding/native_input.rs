//! Applies negotiated packet intents to the client's ordinary movement pipeline.
use super::ModControlModes;
use bevy::prelude::*;
use mod_api::native_camera::{Effects, LookIntent};

/// Synthesized buttons are released through the same finalized input as physical controls.
#[derive(Default, Resource)]
pub(super) struct NativeInput {
    pub(super) scope: Option<(u64, i32)>,
    pub(super) movement: semantic_input::finalized::MovementOverlay,
    /// Personal mods' sprint and sneak intents, in the world scope that held them.
    intents: semantic_input::finalized::MovementOverlay,
    intents_scope: Option<(u64, i32)>,
    controls: semantic_input::finalized::MovementOverlay,
    controls_scope: Option<(u64, i32)>,
}

impl NativeInput {
    /// Replaces axes and adds button holds within one world; omission retires owned buttons.
    fn apply_controls(
        &mut self,
        input: &mut semantic_input::finalized::FinalizedInput,
        scope: Option<(u64, i32)>,
        controls: Option<mod_host::GameplayMovementInput>,
    ) {
        let controls = controls.filter(|_| scope.is_some());
        if self
            .controls_scope
            .zip(scope)
            .is_some_and(|(old, new)| old != new)
        {
            self.controls = Default::default();
        }
        self.controls_scope = scope.or(self.controls_scope);
        if let Some(controls) = controls {
            self.controls.apply(
                input,
                Some((
                    [controls.right, controls.forward],
                    [controls.jump, controls.sneak, controls.sprint],
                )),
            );
        } else {
            self.controls.hold(input, [false; 3]);
        }
    }

    /// Retires controls through the current world's finalized frame before releasing their owner.
    fn apply(
        &mut self,
        input: &mut semantic_input::finalized::FinalizedInput,
        scope: Option<(u64, i32)>,
        intent: Option<([f32; 2], [bool; 3])>,
    ) {
        if self.scope.zip(scope).is_some_and(|(old, new)| old != new) {
            self.movement = Default::default();
        }
        self.scope = scope;
        self.movement.apply(input, intent);
    }

    /// Holds personal-mod sprint and sneak intents over this frame's bindings. Leaving input
    /// ownership releases them; a different world starts without the previous world's holds.
    fn hold_intents(
        &mut self,
        input: &mut semantic_input::finalized::FinalizedInput,
        scope: Option<(u64, i32)>,
        intents: Option<mod_host::GameplayMovementIntents>,
    ) {
        if self
            .intents_scope
            .zip(scope)
            .is_some_and(|(old, new)| old != new)
        {
            self.intents = Default::default();
        }
        self.intents_scope = scope.or(self.intents_scope);
        let intents = intents.filter(|_| scope.is_some());
        self.intents.hold(
            input,
            [
                false,
                intents.is_some_and(|intents| intents.sneak),
                intents.is_some_and(|intents| intents.sprint),
            ],
        );
    }
}

impl ModControlModes {
    /// The player's gameplay settings with this frame's mod modes in place of their own.
    pub(crate) fn apply(&self, mut gameplay: ui::GameplaySettings) -> ui::GameplaySettings {
        if let Some(modes) = self.0 {
            gameplay.toggle_sprint = modes.toggle_sprint;
            gameplay.toggle_sneak = modes.toggle_sneak;
            gameplay.always_sprint = modes.always_sprint;
        }
        gameplay
    }
}

/// Computes an absolute target's bounded step from the current host pose and frame duration.
pub(super) fn look_delta(
    look: LookIntent,
    yaw: f32,
    pitch: f32,
    seconds: f32,
) -> mod_host::CameraDelta {
    let error = Vec2::new(
        (look.yaw - yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
            - std::f32::consts::PI,
        look.pitch - pitch,
    );
    let step = error.clamp_length_max(look.speed * seconds.clamp(0.0, 0.1));
    mod_host::CameraDelta {
        yaw: step.x,
        pitch: step.y,
    }
}

impl super::gameplay::GameplayContext<'_> {
    /// Applies a granted complete control vector while walking owns input; omission releases it.
    pub(super) fn apply_movement_input(
        &mut self,
        controls: Option<mod_host::GameplayMovementInput>,
        input_owned: bool,
        ui: &client_ui::ui_runtime::UiRuntime,
    ) {
        let scope = self.walking_scope(ui).filter(|_| {
            input_owned
                && self
                    .input
                    .as_ref()
                    .and_then(|input| input.snapshot())
                    .is_some_and(|input| !input.authority_released)
        });
        let controls = controls.filter(|_| scope.is_some());
        if let Some(published) = self.movement_override.as_mut() {
            published.0 = controls.is_some();
        }
        let (Some(state), Some(input)) = (self.native_input.as_mut(), self.input.as_mut()) else {
            return;
        };
        state.apply_controls(input, scope, controls);
    }

    /// Both native callback adapters finish camera rotation before resolving world-relative walking.
    pub(super) fn apply_native_controls(
        &mut self,
        effects: Option<&Effects>,
        delta: Option<mod_host::CameraDelta>,
        ui: &client_ui::ui_runtime::UiRuntime,
    ) {
        self.apply_native_look(effects);
        if let Some(delta) = delta {
            self.apply(delta);
        }
        self.apply_native_input(effects, ui);
    }

    /// Native walking never takes a mounted, detached, UI-owned or manually moving body.
    pub(super) fn apply_native_input(
        &mut self,
        effects: Option<&Effects>,
        ui: &client_ui::ui_runtime::UiRuntime,
    ) {
        let scope = self.walking_scope(ui);
        let Some(state) = self.native_input.as_mut() else {
            return;
        };
        let Some(input) = self.input.as_mut() else {
            return;
        };
        let manual = input.raw_movement() != [0.0; 2]
            || input.phase(semantic_input::Action::Jump).held
            || input.phase(semantic_input::Action::Sneak).held
            || input
                .snapshot()
                .is_none_or(|input| input.authority_released);
        let movement = effects
            .and_then(|effects| effects.movement)
            .filter(|_| scope.is_some() && !manual);
        let intent = movement.zip(self.view.as_ref()).map(|(movement, view)| {
            let (yaw, _, _) = view.rotation().to_euler(EulerRot::YXZ);
            let angle = movement.yaw - yaw;
            let (sin, cos) = angle.sin_cos();
            let [x, z] = movement.axes;
            (
                [cos * x - sin * z, sin * x + cos * z],
                [movement.jump, movement.sneak, movement.sprint],
            )
        });
        state.apply(input, scope, intent);
    }

    /// Holds sprint and sneak as if bound while gameplay owns input; omission releases them.
    pub(super) fn apply_movement_intents(
        &mut self,
        intents: Option<mod_host::GameplayMovementIntents>,
        input_owned: bool,
    ) {
        let scope = self
            .world
            .as_ref()
            .and_then(|world| world.stream.as_ref())
            .map(|stream| {
                let authority = stream.authority();
                (authority.actor_session_id(), authority.current_dimension())
            })
            .filter(|_| input_owned);
        let (Some(state), Some(input)) = (self.native_input.as_mut(), self.input.as_mut()) else {
            return;
        };
        let scope = scope.filter(|_| {
            input
                .snapshot()
                .is_some_and(|input| !input.authority_released)
        });
        state.hold_intents(input, scope, intents);
    }

    /// Publishes the control modes mods renewed this frame; omission restores the player's own.
    pub(super) fn apply_control_modes(&mut self, modes: Option<mod_host::GameplayControlModes>) {
        if let Some(published) = self.control_modes.as_mut() {
            published.0 = modes;
        }
    }

    /// Applies absolute look at render cadence, rather than consuming asynchronous relative steps.
    pub(super) fn apply_native_look(&mut self, effects: Option<&Effects>) {
        if let Some(look) = effects.and_then(|effects| effects.look) {
            let Some(view) = self.view.as_ref() else {
                return;
            };
            let (yaw, pitch, _) = view.rotation().to_euler(EulerRot::YXZ);
            let seconds = self
                .real_time
                .as_ref()
                .map_or(0.0, |time| time.delta_secs());
            self.apply(look_delta(look, yaw, pitch, seconds));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retained_look_advances_on_every_host_frame_without_overshoot() {
        let target = LookIntent {
            yaw: 0.5,
            pitch: -0.2,
            speed: 2.0,
        };
        for fps in [30, 60, 144] {
            let (mut yaw, mut pitch) = (0.0, 0.0);
            for _ in 0..fps {
                let delta = look_delta(target, yaw, pitch, 1.0 / fps as f32);
                assert!(delta.yaw.hypot(delta.pitch) <= 2.0 / fps as f32 + 1e-6);
                yaw += delta.yaw;
                pitch += delta.pitch;
            }
            assert!((yaw - target.yaw).abs() < 1e-5);
            assert!((pitch - target.pitch).abs() < 1e-5);
        }
        let delta = look_delta(
            LookIntent {
                yaw: -3.13,
                pitch: 0.0,
                speed: 2.0,
            },
            3.13,
            0.0,
            1.0 / 60.0,
        );
        assert!(delta.yaw > 0.0 && delta.yaw < 0.03);
    }
    /// Control modes replace the player's three control settings only while present.
    #[test]
    fn control_modes_replace_the_player_settings_only_while_present() {
        let player = ui::GameplaySettings {
            toggle_sprint: false,
            always_sprint: true,
            toggle_sneak: true,
            ..Default::default()
        };
        assert_eq!(ModControlModes(None).apply(player), player);
        let modes = mod_host::GameplayControlModes {
            toggle_sprint: true,
            toggle_sneak: false,
            always_sprint: false,
        };
        assert_eq!(
            ModControlModes(Some(modes)).apply(player),
            ui::GameplaySettings {
                toggle_sprint: true,
                toggle_sneak: false,
                always_sprint: false,
                ..player
            }
        );
    }

    /// Each frame publishes the merged modes it was given, so omission restores the player's own.
    #[test]
    fn published_control_modes_are_replaced_every_frame() {
        use bevy::ecs::system::RunSystemOnce;
        let modes = mod_host::GameplayControlModes {
            toggle_sprint: true,
            toggle_sneak: true,
            always_sprint: false,
        };
        let mut world = World::new();
        world.init_resource::<ModControlModes>();
        world
            .run_system_once(
                move |mut gameplay: super::super::gameplay::GameplayContext| {
                    gameplay.apply_control_modes(Some(modes));
                },
            )
            .unwrap();
        assert_eq!(world.resource::<ModControlModes>().0, Some(modes));
        world
            .run_system_once(|mut gameplay: super::super::gameplay::GameplayContext| {
                gameplay.apply_control_modes(None);
            })
            .unwrap();
        assert_eq!(world.resource::<ModControlModes>().0, None);
    }

    /// A retained override must be cleared when no gameplay world or input owner is present.
    #[test]
    fn movement_input_releases_without_gameplay_ownership() {
        use super::super::ModMovementInputActive;
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        world.insert_resource(ModMovementInputActive(true));
        world
            .run_system_once(|mut gameplay: super::super::gameplay::GameplayContext| {
                let ui = client_ui::ui_runtime::UiRuntime::new(7);
                gameplay.apply_movement_input(
                    Some(mod_host::GameplayMovementInput {
                        right: 1.0,
                        forward: 0.0,
                        jump: false,
                        sneak: false,
                        sprint: true,
                    }),
                    true,
                    &ui,
                );
            })
            .unwrap();
        assert!(!world.resource::<ModMovementInputActive>().0);
    }

    /// Complete vectors remap axes and independent buttons, then retire cleanly on omission.
    #[test]
    fn complete_controls_replace_axes_add_buttons_and_retire_by_scope() {
        let fresh = || {
            let mut router = semantic_input::SemanticInputRouter::default();
            router
                .route(semantic_input::DeviceFrame::default())
                .unwrap();
            semantic_input::finalized::FinalizedInput::from_finalized(router.finalize().unwrap())
        };
        let mut state = NativeInput::default();
        let controls = mod_host::GameplayMovementInput {
            right: -0.5,
            forward: 0.75,
            jump: true,
            sneak: true,
            sprint: false,
        };
        let mut frame = fresh();
        state.apply_controls(&mut frame, Some((1, 0)), Some(controls));
        assert_eq!(frame.movement(), [-0.5, 0.75]);
        assert!(frame.movement_is_resolved());
        assert!(frame.phase(semantic_input::Action::Jump).pressed);
        assert!(frame.phase(semantic_input::Action::Sneak).held);
        assert!(!frame.phase(semantic_input::Action::Sprint).held);
        let mut next = fresh();
        state.apply_controls(
            &mut next,
            Some((1, 0)),
            Some(mod_host::GameplayMovementInput {
                right: 0.25,
                forward: -1.0,
                jump: false,
                sneak: false,
                sprint: true,
            }),
        );
        assert_eq!(next.movement(), [0.25, -1.0]);
        assert!(next.phase(semantic_input::Action::Jump).released);
        assert!(next.phase(semantic_input::Action::Sprint).pressed);
        let mut omitted = fresh();
        state.apply_controls(&mut omitted, Some((1, 0)), None);
        assert_eq!(omitted.movement(), [0.0; 2]);
        assert!(!omitted.movement_is_resolved());
        assert!(omitted.phase(semantic_input::Action::Sprint).released);
        state.apply_controls(&mut fresh(), Some((1, 0)), Some(controls));
        let mut other_world = fresh();
        state.apply_controls(&mut other_world, Some((2, 0)), None);
        assert!(!other_world.phase(semantic_input::Action::Jump).released);
        let mut no_owner = fresh();
        state.apply_controls(&mut no_owner, None, Some(controls));
        assert_eq!(no_owner.movement(), [0.0; 2]);
        assert!(!no_owner.phase(semantic_input::Action::Jump).held);
    }

    #[test]
    fn personal_intents_hold_sprint_and_sneak_and_release_outside_their_scope() {
        let fresh = || {
            let mut router = semantic_input::SemanticInputRouter::default();
            router
                .route(semantic_input::DeviceFrame::default())
                .unwrap();
            semantic_input::finalized::FinalizedInput::from_finalized(router.finalize().unwrap())
        };
        let intents = Some(mod_host::GameplayMovementIntents {
            sprint: true,
            sneak: false,
        });
        let mut state = NativeInput::default();
        let mut held = fresh();
        state.hold_intents(&mut held, Some((1, 0)), intents);
        let sprint = held.phase(semantic_input::Action::Sprint);
        assert!(sprint.pressed && sprint.held);
        assert!(!held.phase(semantic_input::Action::Sneak).held);
        assert_eq!(held.raw_movement(), [0.0; 2]);
        let mut lost = fresh();
        state.hold_intents(&mut lost, None, intents);
        assert!(lost.phase(semantic_input::Action::Sprint).released);
        assert!(!lost.phase(semantic_input::Action::Sprint).held);
        let mut renewed = fresh();
        state.hold_intents(&mut renewed, Some((1, 0)), intents);
        let mut omitted = fresh();
        state.hold_intents(&mut omitted, Some((1, 0)), None);
        assert!(omitted.phase(semantic_input::Action::Sprint).released);
        state.hold_intents(&mut fresh(), Some((1, 0)), intents);
        let mut next_world = fresh();
        state.hold_intents(&mut next_world, Some((2, 0)), None);
        assert!(!next_world.phase(semantic_input::Action::Sprint).released);
    }

    #[test]
    fn losing_walking_eligibility_publishes_the_retiring_buttons() {
        let fresh = || {
            let mut router = semantic_input::SemanticInputRouter::default();
            router
                .route(semantic_input::DeviceFrame::default())
                .unwrap();
            semantic_input::finalized::FinalizedInput::from_finalized(router.finalize().unwrap())
        };
        let mut state = NativeInput::default();
        state.apply(
            &mut fresh(),
            Some((1, 0)),
            Some(([0.0, 1.0], [true, true, false])),
        );
        let mut released = fresh();
        state.apply(&mut released, None, None);
        assert!(released.phase(semantic_input::Action::Jump).released);
        assert!(released.phase(semantic_input::Action::Sneak).released);
        let mut next_world = fresh();
        state.apply(&mut next_world, Some((2, 0)), None);
        assert!(!next_world.phase(semantic_input::Action::Jump).released);
    }
}
