//! Offline integration of guest requests, the frame driver and native delivery.

use super::*;
use bevy::{
    input::mouse::AccumulatedMouseMotion, math::DVec2, time::Real, window::WindowResolution,
};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Recorded {
    captured: bool,
    cached: Vec2,
    ordinary_writes: usize,
    explicit: Vec<DVec2>,
}

struct Recorder(Arc<Mutex<Recorded>>);
impl NativePointer for Recorder {
    /// Records the physical write only after the backend has released capture.
    fn position(&mut self, _: Entity, point: DVec2) -> Result<(), String> {
        let mut recorded = self.0.lock().unwrap();
        assert!(!recorded.captured, "native move preceded capture release");
        recorded.explicit.push(point);
        Ok(())
    }
}

#[derive(Resource)]
struct WindowBackend(Arc<Mutex<Recorded>>);

/// Models the backend's cached position update followed by its native capture update.
fn update_backend(
    window: Single<(&Window, &CursorOptions), With<PrimaryWindow>>,
    backend: Res<WindowBackend>,
) {
    let mut recorded = backend.0.lock().unwrap();
    if let Some(point) = window.0.cursor_position()
        && point != recorded.cached
    {
        recorded.ordinary_writes += 1;
        recorded.cached = point;
    }
    recorded.captured = window.1.grab_mode != CursorGrabMode::None;
}

/// Creates the actual presentation and input resources without a native window or game assets.
fn app() -> (App, Entity, Arc<Mutex<Recorded>>) {
    let mut player = client_state::PlayerRuntime(player_state::PlayerState::new(1));
    let mut ui = client_ui::test_support::inventory_session(&mut player);
    ui.toggle_inventory(&mut player, None);
    let stream = chunk_pipeline::WorldStream::new(protocol::WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.; 3],
        world_spawn_position: [0; 3],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    });
    let mut app = App::new();
    app.insert_resource(crate::runtime::world::ClientWorld {
        stream: Some(stream),
        ..Default::default()
    });
    app.add_message::<bevy::input::keyboard::KeyboardInput>()
        .add_message::<bevy::input::mouse::MouseButtonInput>()
        .add_message::<bevy::input::mouse::MouseWheel>()
        .init_resource::<Time<Real>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .init_resource::<crate::modding::packet_render::Drawing>()
        .init_resource::<render_scene::mod_scene::retained::DrawingFrame>()
        .insert_resource(player)
        .insert_resource(UiRuntimeResource(ui))
        .insert_resource(UiPresentationResource(
            client_ui::ui_runtime::presentation::UiPresentationRuntime::new(
                ui_engine::test_support::fixture_font(),
            )
            .unwrap(),
        ));
    let mut window = Window {
        focused: true,
        resolution: WindowResolution::new(1280, 800),
        ..Default::default()
    };
    let center = window.size() * 0.5;
    window.set_cursor_position(Some(center));
    let entity = app
        .world_mut()
        .spawn((window, CursorOptions::default(), PrimaryWindow))
        .id();
    let recorded = Arc::new(Mutex::new(Recorded {
        captured: true,
        cached: center,
        ..Default::default()
    }));
    configure(&mut app);
    app.insert_resource(Backend(Box::new(Recorder(recorded.clone()))))
        .insert_resource(WindowBackend(recorded.clone()))
        .add_systems(Last, update_backend);
    (app, entity, recorded)
}

/// Loads a real guest that requests the center once, retrying denied requests.
fn host(app: &App, granted: bool) -> ModHost {
    let world = app.world().resource::<crate::runtime::world::ClientWorld>();
    let connection = world
        .stream
        .as_ref()
        .unwrap()
        .authority()
        .actor_session_id();
    let bytes = wat::parse_str(include_str!("probe.wat")).unwrap();
    let path = std::path::Path::new("pointer-probe.wasm");
    let host = ModHost::prepare_snapshot_in_session(
        path,
        &bytes,
        ModGrants {
            controls: granted,
            ..Default::default()
        },
        None,
        Some(mod_api::drawing::DrawingSession {
            connection,
            permissions: mod_api::drawing::DrawingPermissions::ALL,
            ..Default::default()
        }),
    )
    .unwrap();
    assert!(host.is_active());
    assert_eq!(host.grants().controls, granted);
    host
}

/// Drives the production guest callbacks and stages their actual committed requests.
fn install_driver(app: &mut App, host: ModHost) {
    app.insert_resource(ModRuntime::new(
        host,
        registration::HostMetadata::environment(0),
    ))
    .insert_resource(crate::environment::VisualTimeOverride(None))
    .init_resource::<interaction::ModInteraction>()
    .add_systems(Update, (begin_mod_frame, drive_mod).chain());
}

/// Empty startup must run the installed production native system without requiring a mod.
#[test]
fn starts_without_loaded_mods() {
    let (mut app, _, recorded) = app();
    app.update();
    assert!(recorded.lock().unwrap().explicit.is_empty());
}

/// A real guest's equal-coordinate request bypasses the cache after release, exactly once.
#[test]
fn guest_request_reaches_native_after_capture_release_even_when_cached() {
    for scale in [1., 2.] {
        let (mut app, window, recorded) = app();
        app.world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .resolution
            .set_scale_factor_override(Some(scale));
        let center = app.world().get::<Window>(window).unwrap().size() * 0.5;
        app.world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .set_cursor_position(Some(center));
        recorded.lock().unwrap().cached = center;
        let guest = host(&app, true);
        install_driver(&mut app, guest);
        app.update();
        app.update();
        let recorded = recorded.lock().unwrap();
        assert_eq!(
            recorded.ordinary_writes, 0,
            "the cached path alone must reproduce the lost move"
        );
        assert_eq!(recorded.explicit, vec![DVec2::new(640., 400.)]);
    }
}

/// Capture holds delay a real request; removal of the runtime cancels it without a panic.
#[test]
fn waits_for_capture_and_cancels_on_last_mod_unload() {
    for unload in [false, true] {
        let (mut app, window, recorded) = app();
        app.world_mut()
            .get_mut::<CursorOptions>(window)
            .unwrap()
            .grab_mode = CursorGrabMode::Locked;
        let guest = host(&app, true);
        install_driver(&mut app, guest);
        app.update();
        assert!(
            app.world().resource::<Pending>().0.is_some(),
            "active={} revoked={} fuel={}",
            app.world()
                .resource::<ModRuntime>()
                .hosts
                .host(0)
                .is_active(),
            app.world()
                .resource::<ModRuntime>()
                .hosts
                .host(0)
                .is_revoked(),
            app.world()
                .resource::<ModRuntime>()
                .hosts
                .host(0)
                .last_fuel_used()
        );
        assert!(recorded.lock().unwrap().explicit.is_empty());
        if unload {
            app.world_mut().remove_resource::<ModRuntime>();
        }
        app.world_mut()
            .get_mut::<CursorOptions>(window)
            .unwrap()
            .grab_mode = CursorGrabMode::None;
        app.update();
        assert!(app.world().resource::<Pending>().0.is_none());
        assert_eq!(
            recorded.lock().unwrap().explicit.len(),
            usize::from(!unload)
        );
    }
}

/// Authority changes between the callback and native delivery cannot move the desktop.
#[test]
fn cancels_waiting_requests_on_authority_departure() {
    for change in [
        "screen",
        "focus",
        "window",
        "suspended",
        "replacement",
        "revoked",
        "panel",
        "driven",
        "inactive",
        "hidden",
    ] {
        let (mut app, window, recorded) = app();
        app.world_mut()
            .get_mut::<CursorOptions>(window)
            .unwrap()
            .grab_mode = CursorGrabMode::Locked;
        let guest = host(&app, true);
        install_driver(&mut app, guest);
        app.update();
        assert!(app.world().resource::<Pending>().0.is_some(), "{change}");
        // The driver was already exercised. Cancel before the real native commit, without
        // letting a replacement guest submit an independent request in the next update.
        match change {
            "screen" => {
                let world = app.world_mut();
                world.resource_scope(|world, mut player: Mut<client_state::PlayerRuntime>| {
                    world
                        .resource_mut::<UiRuntimeResource>()
                        .toggle_inventory(&mut player, None);
                });
            }
            "focus" => app.world_mut().get_mut::<Window>(window).unwrap().focused = false,
            "window" => {
                app.world_mut().despawn(window);
            }
            "suspended" => app.world_mut().resource_mut::<ModRuntime>().suspended = true,
            "replacement" | "revoked" => {
                let replacement = host(&app, change != "revoked");
                app.world_mut().insert_resource(ModRuntime::new(
                    replacement,
                    registration::HostMetadata::environment(0),
                ));
            }
            "panel" => {
                let mut runtime = app.world_mut().resource_mut::<ModRuntime>();
                let host = runtime.hosts.host_mut(0);
                host.set_panel_bundle(&serde_json::json!({"panel":{"controls":[],"surface":{
                    "screen":"fixture.root", "document":"{\"namespace\":\"fixture\",\"root\":{\"type\":\"panel\"}}"
                }}, "surfaces":{}}).to_string()).unwrap();
                host.set_panel_open(true);
            }
            "inactive" => {
                app.world_mut()
                    .resource_mut::<ModRuntime>()
                    .hosts
                    .host_mut(0)
                    .bind_drawing_session(mod_api::drawing::DrawingSession {
                        connection: 1,
                        ..Default::default()
                    });
            }
            "hidden" => {
                app.world_mut()
                    .get_mut::<CursorOptions>(window)
                    .unwrap()
                    .visible = false;
            }
            "driven" => {
                app.world_mut().insert_resource(client_state::DrivenInput);
            }
            _ => unreachable!(),
        }
        if change != "window" {
            app.world_mut()
                .get_mut::<CursorOptions>(window)
                .unwrap()
                .grab_mode = CursorGrabMode::None;
        }
        app.world_mut().run_schedule(ApplyNativePointer);
        assert!(app.world().resource::<Pending>().0.is_none(), "{change}");
        assert!(recorded.lock().unwrap().explicit.is_empty(), "{change}");
    }
}

/// The driver never queues native writes for a denied grant or synthetic pointer ownership.
#[test]
fn driver_keeps_denied_and_driven_requests_off_the_desktop() {
    for driven in [false, true] {
        let (mut app, _, recorded) = app();
        let guest = host(&app, driven);
        install_driver(&mut app, guest);
        if driven {
            app.world_mut().insert_resource(client_state::DrivenInput);
        }
        app.update();
        assert!(app.world().resource::<Pending>().0.is_none());
        assert!(recorded.lock().unwrap().explicit.is_empty());
    }
}
