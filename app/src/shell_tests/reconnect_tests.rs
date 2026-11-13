use client_shell::menu::MenuRuntime;
use client_shell::menu::test_support as menu_fixture;
use launcher::menu::{MenuAction, MenuScreen};

/// A launcher menu with a requested join that cannot reach a core, and the session systems
/// that act on it. Also returns the previous session's control channel.
fn joining_app() -> (bevy::prelude::App, tokio::sync::mpsc::Sender<impl Sized>) {
    use crate::runtime::{network::resource_packs::PackApplication, world::ClientWorld};
    use crate::session::{SessionController, drive_session};
    use bevy::prelude::{App, AppExit, Update};
    use client_runtime::{
        blob_cache::ClientBlobCacheOwner, network::NetworkHandle,
        pack_admission::ResourcePackAdmissionState,
    };
    use client_shell::ui_resources::UiRuntimeResource;
    use client_ui::ui_runtime::UiRuntime;

    let mut menu = MenuRuntime::new(true, 2, "Player".into());
    menu_fixture::suppress_catalog(&mut menu);
    menu_fixture::use_missing_core(&mut menu);
    menu.activate(MenuAction::Navigate(MenuScreen::Servers));
    menu.request_connect("127.0.0.1:19132".into());
    let mut network = NetworkHandle::<PackApplication>::disconnected();
    let (old_controls, receiver) = tokio::sync::mpsc::channel(1);
    *network.control_events_mut() = receiver;
    let mut app = App::new();
    app.add_message::<AppExit>()
        .insert_resource(menu)
        .insert_resource(SessionController::default())
        .insert_resource(network)
        .insert_resource(ClientBlobCacheOwner::default())
        .insert_resource(ResourcePackAdmissionState::default())
        .insert_resource(crate::runtime::network::session_registries::tests::collision_resource())
        .insert_resource(UiRuntimeResource(UiRuntime::new(1)))
        .insert_resource(client_state::PlayerRuntime(player_state::PlayerState::new(
            1,
        )))
        .insert_resource(ClientWorld::default())
        .init_resource::<crate::runtime::shutdown::FatalError>()
        .insert_resource(client_interaction::movement::MovementTicker::default())
        .insert_resource(client_interaction::movement::LocalPhysicsController::default())
        .insert_resource(client_presentation::local_player::LocalPlayerFrameCarrier::default())
        .insert_resource(client_presentation::local_player::InteractionOriginSnapshot::default())
        .add_systems(Update, drive_session);
    (app, old_controls)
}

#[test]
fn reconnect_drives_two_fresh_production_attempts_and_retires_old_controls() {
    use crate::runtime::world::ClientWorld;
    use client_shell::ui_resources::UiRuntimeResource;

    let (mut app, old_controls) = joining_app();
    for attempt in 0..2 {
        let previous = app.world().resource::<UiRuntimeResource>().session_id();
        app.update();
        assert!(app.world().resource::<UiRuntimeResource>().session_id() > previous);
        assert!(old_controls.is_closed());
        let menu = app.world().resource::<MenuRuntime>();
        assert!(!menu.is_connecting());
        assert!(menu.view().can_reconnect);
        assert!(
            menu.view()
                .disconnect_message
                .as_ref()
                .unwrap()
                .contains("127.0.0.1:19132")
        );
        assert!(app.world().resource::<ClientWorld>().stream.is_none());
        if attempt == 0 {
            app.world_mut()
                .resource_mut::<MenuRuntime>()
                .activate_focused();
        }
    }
}

/// Platforms that keep world shaders out of the menu start compiling them when a join starts,
/// even one that then fails to reach a core.
#[test]
fn a_join_request_admits_world_shader_work_that_waited_for_it() {
    use render_scene::startup_work::{WorldGpuStart, WorldWorkAdmission};

    for start in [WorldGpuStart::OnJoin, WorldGpuStart::AfterMenu] {
        let admission = WorldWorkAdmission::after_menu(start);
        admission.menu_presented();
        assert_eq!(admission.is_admitted(), start == WorldGpuStart::AfterMenu);
        let (mut app, _old_controls) = joining_app();
        app.insert_resource(admission.clone());
        app.update();
        assert!(admission.is_admitted());
    }
}
