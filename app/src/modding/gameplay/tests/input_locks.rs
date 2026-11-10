use super::*;
use protocol::{ClientInputLock, ClientInputLocks};

struct Floor;
impl sim::CollisionWorld for Floor {
    /// Supplies loaded ground so a wrongly retained jump can actually initiate.
    fn collision_boxes(
        &self,
        query: sim::Aabb,
    ) -> Result<sim::CollisionQuery<Vec<sim::Aabb>>, sim::WorldQueryError> {
        let floor = sim::Aabb::new(
            sim::Vec3::new(-16.0, 0.0, -16.0),
            sim::Vec3::new(16.0, 1.0, 16.0),
        );
        Ok(sim::CollisionQuery::synthetic(
            floor
                .intersects(query)
                .then_some(floor)
                .into_iter()
                .collect(),
        ))
    }
}

#[test]
fn server_input_locks_gate_mod_camera_deltas_and_allow_fresh_deltas_after_unlock() {
    /// Applies a committed extension output through its production system parameter.
    fn rotate(mut context: GameplayContext) {
        context.apply(CameraDelta {
            yaw: 0.3,
            pitch: 0.2,
        });
    }
    let mut app = App::new();
    let initial = Quat::from_rotation_y(0.4);
    app.insert_resource(client_state::PlayerRuntime(player_state::PlayerState::new(
        7,
    )))
    .insert_resource(LocalViewPose::new(Vec3::Y * 70.0, initial))
    .add_systems(Update, rotate);
    for (bits, blocked) in [
        (ClientInputLock::Camera as u32, true),
        (0, false),
        (ClientInputLock::Movement as u32, false),
    ] {
        app.world_mut()
            .resource_mut::<LocalViewPose>()
            .set_rotation(initial);
        app.world_mut()
            .resource_mut::<client_state::PlayerRuntime>()
            .facts
            .apply_input_locks(7, ClientInputLocks::from_bits(bits));
        app.update();
        assert_eq!(
            app.world().resource::<LocalViewPose>().rotation() == initial,
            blocked
        );
    }
}

#[test]
fn gameplay_jump_button_survives_subtick_frames_and_obeys_input_locks() {
    use client_interaction::movement::{LocalPhysicsController, MovementTicker};
    use client_shell::ui_resources::UiRuntimeResource;
    use gameplay::movement::MovementSource;

    #[derive(Resource, Default)]
    struct ObservedEligibility(bool);

    /// Runs the mod admission calls without advancing physics in this frame.
    fn request_jump(
        mut context: GameplayContext,
        ui: Res<UiRuntimeResource>,
        runtime: Res<crate::modding::ModRuntime>,
        mut observed: ResMut<ObservedEligibility>,
    ) {
        let allowed = jump_input_allowed(&runtime.hosts, false, true);
        context.synchronize_jump_scope(allowed, &ui);
        observed.0 = context
            .movement_snapshot(
                true,
                &ModGrants {
                    movement: true,
                    ..Default::default()
                },
                &ui,
            )
            .is_some();
        context.pulse_button(mod_host::GameplayButton::Jump, None, &ui);
    }

    for (bits, blocked) in [
        (ClientInputLock::Jump as u32, true),
        (ClientInputLock::Movement as u32, true),
        (ClientInputLock::Camera as u32, false),
        (0, false),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("jump-authority.wat");
        let component = br#"(component
            (core module $m (func (export "init")) (func (export "frame")))
            (core instance $i (instantiate $m))
            (func (export "init") (canon lift (core func $i "init")))
            (func (export "frame") (canon lift (core func $i "frame"))))"#;
        let host = mod_host::ModHost::load_snapshot_with_grants(
            &path,
            component,
            ModGrants {
                gameplay_input: true,
                ..Default::default()
            },
        )
        .unwrap();
        let mut app = App::new();
        let mut physics = LocalPhysicsController::default();
        let position = [0.0, 1.0 + protocol::PLAYER_NETWORK_OFFSET, 0.0];
        physics.reanchor_network_position(position, 100, true);
        let mut movement = MovementTicker::default();
        movement.reset(7, 100, position);
        movement.set_source(MovementSource::Physics);
        let mut player = client_state::PlayerRuntime(player_state::PlayerState::new(7));
        player
            .facts
            .apply_input_locks(7, ClientInputLocks::from_bits(bits));
        app.insert_resource(player)
            .insert_resource(crate::modding::ModRuntime::new(
                host,
                crate::modding::registration::HostMetadata::environment(0),
            ))
            .insert_resource(physics)
            .insert_resource(movement)
            .insert_resource(finalized_input(false))
            .insert_resource(ClientWorld {
                stream: Some(stream()),
                ..Default::default()
            })
            .insert_resource(UiRuntimeResource(client_ui::ui_runtime::UiRuntime::new(7)))
            .init_resource::<ObservedEligibility>()
            .add_systems(Update, request_jump);
        app.update();
        assert_eq!(app.world().resource::<ObservedEligibility>().0, !blocked);
        app.world_mut()
            .resource_mut::<client_state::PlayerRuntime>()
            .facts
            .apply_input_locks(7, ClientInputLocks::default());
        let mut physics = app.world_mut().resource_mut::<LocalPhysicsController>();
        let early = physics.advance(
            Duration::from_millis(10),
            sim::MovementInput::default(),
            &Floor,
        );
        assert!(early.samples.is_empty());
        let result = physics.advance(
            Duration::from_millis(90),
            sim::MovementInput::default(),
            &Floor,
        );
        assert_eq!(result.samples.len(), 2);
        assert_eq!(result.samples[0].processed.jump_initiated, !blocked);
        assert_eq!(result.samples[0].input.jump.held, !blocked);
        assert_eq!(result.samples[0].input.jump.pressed, !blocked);
        assert!(!result.samples[1].input.jump.held);
        assert!(!result.samples[1].input.jump.pressed);
        assert_eq!(result.samples[1].input.jump.released, !blocked);
    }
}

#[test]
fn released_input_rejects_new_jump_taps_and_revokes_pending_pulses() {
    use bevy::ecs::system::RunSystemOnce;
    use client_interaction::movement::{LocalPhysicsController, MovementTicker};
    use client_shell::ui_resources::UiRuntimeResource;
    use gameplay::movement::MovementSource;

    for pending in [false, true] {
        let mut app = App::new();
        let mut physics = LocalPhysicsController::default();
        let position = [0.0, 1.0 + protocol::PLAYER_NETWORK_OFFSET, 0.0];
        physics.reanchor_network_position(position, 100, true);
        let mut movement = MovementTicker::default();
        movement.reset(7, 100, position);
        movement.set_source(MovementSource::Physics);
        app.insert_resource(client_state::PlayerRuntime(player_state::PlayerState::new(
            7,
        )))
        .insert_resource(physics)
        .insert_resource(movement)
        .insert_resource(finalized_input(false))
        .insert_resource(ClientWorld {
            stream: Some(stream()),
            ..Default::default()
        })
        .insert_resource(UiRuntimeResource(client_ui::ui_runtime::UiRuntime::new(7)));
        app.world_mut()
            .run_system_once(
                move |mut context: GameplayContext, ui: Res<UiRuntimeResource>| {
                    context.synchronize_jump_scope(true, &ui);
                    context.pulse_jump(pending, &ui);
                },
            )
            .unwrap();

        let mut released = app
            .world()
            .resource::<SemanticInputSnapshot>()
            .snapshot()
            .unwrap()
            .clone();
        released.authority_released = true;
        app.world_mut()
            .resource_mut::<SemanticInputSnapshot>()
            .replace(released);
        app.world_mut()
            .run_system_once(
                move |mut context: GameplayContext, ui: Res<UiRuntimeResource>| {
                    if pending {
                        context.synchronize_jump_scope(true, &ui);
                    } else {
                        context.pulse_button(mod_host::GameplayButton::Jump, None, &ui);
                    }
                },
            )
            .unwrap();
        let frame = app
            .world_mut()
            .resource_mut::<LocalPhysicsController>()
            .advance(
                Duration::from_millis(50),
                sim::MovementInput::default(),
                &Floor,
            );
        assert_eq!(frame.samples.len(), 1);
        assert!(!frame.samples[0].processed.jump_initiated);
        assert!(!frame.samples[0].input.jump.held);
        assert!(!frame.samples[0].input.jump.pressed);

        app.insert_resource(finalized_input(false));
        app.world_mut()
            .run_system_once(|mut context: GameplayContext, ui: Res<UiRuntimeResource>| {
                context.synchronize_jump_scope(true, &ui);
                context.pulse_button(mod_host::GameplayButton::Jump, None, &ui);
            })
            .unwrap();
        let frame = app
            .world_mut()
            .resource_mut::<LocalPhysicsController>()
            .advance(
                Duration::from_millis(50),
                sim::MovementInput::default(),
                &Floor,
            );
        assert_eq!(frame.samples.len(), 1);
        assert!(frame.samples[0].processed.jump_initiated);
        assert!(frame.samples[0].input.jump.pressed);
    }
}

#[test]
fn native_camera_rotation_and_movement_share_the_final_body_yaw() {
    use client_interaction::movement::{LocalPhysicsController, MovementTicker};
    use client_shell::ui_resources::UiRuntimeResource;
    use gameplay::movement::MovementSource;
    /// Runs the same control application used by both packet callback adapters.
    fn apply(mut context: GameplayContext, ui: Res<UiRuntimeResource>) {
        let effects = mod_api::native_camera::Effects {
            movement: Some(mod_api::native_camera::MovementIntent {
                yaw: 0.0,
                axes: [0.0, 1.0],
                jump: false,
                sneak: false,
                sprint: false,
            }),
            ..Default::default()
        };
        context.apply_native_controls(
            Some(&effects),
            Some(CameraDelta {
                yaw: 0.1,
                pitch: 0.0,
            }),
            &ui,
        );
    }
    let mut physics = LocalPhysicsController::default();
    let position = [0.0, 1.0 + protocol::PLAYER_NETWORK_OFFSET, 0.0];
    physics.reanchor_network_position(position, 100, true);
    let mut movement = MovementTicker::default();
    movement.reset(7, 100, position);
    movement.set_source(MovementSource::Physics);
    let mut app = App::new();
    app.insert_resource(physics)
        .insert_resource(movement)
        .insert_resource(client_state::PlayerRuntime(player_state::PlayerState::new(
            7,
        )))
        .insert_resource(finalized_input(false))
        .insert_resource(LocalViewPose::new(Vec3::ZERO, Quat::IDENTITY))
        .insert_resource(ClientWorld {
            stream: Some(stream()),
            ..Default::default()
        })
        .insert_resource(UiRuntimeResource(client_ui::ui_runtime::UiRuntime::new(7)))
        .init_resource::<super::super::super::native_input::NativeInput>()
        .add_systems(Update, apply);
    app.update();
    let axes = app.world().resource::<SemanticInputSnapshot>().movement();
    let rotation = app.world().resource::<LocalViewPose>().rotation();
    let direction = rotation * Vec3::new(axes[0], 0.0, -axes[1]);
    assert!(
        direction.distance(Vec3::NEG_Z) < 1e-5,
        "relative look changed the requested walking heading: {direction:?}"
    );
}
