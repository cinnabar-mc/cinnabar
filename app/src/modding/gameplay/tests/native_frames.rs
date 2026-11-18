use super::*;
use client_shell::ui_resources::UiRuntimeResource;

#[derive(Resource, Default)]
struct Frames(Vec<mod_host::NativeFrame>);

/// Capture active/suspended frames through the production input/world boundary.
fn capture_native(
    context: GameplayContext,
    request: Res<Request>,
    ui: Res<UiRuntimeResource>,
    mut frames: ResMut<Frames>,
) {
    frames.0.push(
        context
            .native_frame(request.allowed, &ui)
            .expect("connected scope"),
    );
}

#[test]
fn active_and_suspended_frames_share_the_world_identity() {
    let mut app = app();
    let actor = app
        .world()
        .resource::<ClientWorld>()
        .stream
        .as_ref()
        .unwrap()
        .authority()
        .actor_session_id();
    let ui_generation = actor + 100;
    let mut clock = client_presentation::environment::WorldClock::default();
    let mut weather = client_presentation::environment::WeatherState::default();
    client_presentation::environment::bind_session_generation(
        &mut clock,
        &mut weather,
        ui_generation,
    );
    app.insert_resource(clock)
        .insert_resource(UiRuntimeResource(client_ui::ui_runtime::UiRuntime::new(
            ui_generation,
        )))
        .init_resource::<Frames>()
        .add_systems(Update, capture_native);
    for allowed in [true, false, true] {
        app.world_mut().resource_mut::<Request>().allowed = allowed;
        app.update();
    }
    let frames = &app.world().resource::<Frames>().0;
    assert_ne!(actor, ui_generation);
    assert_eq!(
        frames.iter().map(|f| f.suspended).collect::<Vec<_>>(),
        [false, true, false]
    );
    for frame in frames {
        assert_eq!(frame.session, actor, "suspension changed world scope");
    }
}

/// Native frames list only protocol-classified remote players, never the local player or an
/// entity that merely uses the player identifier.
#[test]
fn native_frames_list_remote_protocol_players_only() {
    let mut app = app();
    {
        let mut world = app.world_mut().resource_mut::<ClientWorld>();
        let stream = world.stream.as_mut().unwrap();
        stream.submit(1, player(1, [0.0; 3])).unwrap();
        stream.submit(2, player(2, [2.0, 0.0, 0.0])).unwrap();
        let impostor = ActorKind::Entity {
            identifier: "minecraft:player".into(),
        };
        stream.submit(3, spawn(3, impostor, [0.0; 3])).unwrap();
    }
    let session = app
        .world()
        .resource::<ClientWorld>()
        .stream
        .as_ref()
        .unwrap()
        .authority()
        .actor_session_id();
    let mut clock = client_presentation::environment::WorldClock::default();
    client_presentation::environment::bind_session_generation(
        &mut clock,
        &mut client_presentation::environment::WeatherState::default(),
        session,
    );
    app.insert_resource(clock)
        .insert_resource(UiRuntimeResource(client_ui::ui_runtime::UiRuntime::new(
            session,
        )))
        .init_resource::<Frames>()
        .add_systems(Update, capture_native);
    app.update();
    let frame = &app.world().resource::<Frames>().0[0];
    assert!(!frame.suspended);
    let players: Vec<_> = frame.players.iter().map(|p| p.runtime_id).collect();
    assert_eq!(players, [2]);
}
