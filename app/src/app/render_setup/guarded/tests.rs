use super::*;
use bevy::{
    prelude::{Entity, With},
    window::{PrimaryWindow, WindowCloseRequested, WindowPlugin},
};
use launcher_host::renderer_startup::{Attempt, Stage};
use render_scene::startup_presentation::StartupPresentation;

/// Builds an unfinished first frame without loading a graphics driver.
fn pending_attempt(backend: Backend) -> Attempt {
    Attempt {
        backend,
        adapter: None,
        stage: Stage::FirstPresentation,
        explicit: false,
    }
}

#[test]
fn closing_the_window_before_presentation_does_not_poison_recovery() {
    let dir = tempfile::tempdir().unwrap();
    for _ in 0..2 {
        let mut guard = StartupGuard::open(dir.path()).unwrap();
        assert!(guard.interrupted.is_none());
        assert!(guard.recovery.failed_backends().is_empty());
        assert!(guard.recovery.working.is_none());
        guard.begin(pending_attempt(Backend::Vulkan)).unwrap();
        let mut app = App::new();
        app.add_plugins(WindowPlugin::default())
            .init_resource::<StartupPresentation>();
        install_guard(&mut app, guard);
        let mut windows = app
            .world_mut()
            .query_filtered::<Entity, With<PrimaryWindow>>();
        let window = windows.single(app.world()).unwrap();
        app.world_mut()
            .write_message(WindowCloseRequested { window });
        test_time::eventually("window closure requests application exit", || {
            app.update();
            app.should_exit().is_some()
        });
        assert_eq!(app.should_exit(), Some(AppExit::Success));
        assert!(StartupGuard::pending(dir.path()).unwrap().is_none());
    }
    let next = StartupGuard::open(dir.path()).unwrap();
    assert!(next.interrupted.is_none());
    assert!(next.recovery.failed_backends().is_empty());
    assert!(next.recovery.working.is_none());
}

#[test]
fn interface_startup_failure_does_not_poison_graphics_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let mut guard = StartupGuard::open(dir.path()).unwrap();
    let pending = pending_attempt(Backend::Vulkan);
    guard.begin(pending.clone()).unwrap();
    let mut app = App::new();
    install_guard(&mut app, guard);
    assert!(super::super::finish_prerequisite(&mut app, Ok(())).is_ok());
    assert_eq!(StartupGuard::pending(dir.path()).unwrap(), Some(pending));
    let failure = super::super::finish_prerequisite::<()>(
        &mut app,
        Err(anyhow::anyhow!("invalid required interface carrier")),
    )
    .unwrap_err();
    assert_eq!(failure.to_string(), "invalid required interface carrier");
    assert!(StartupGuard::pending(dir.path()).unwrap().is_none());
    drop(app);
    let next = StartupGuard::open(dir.path()).unwrap();
    assert!(next.interrupted.is_none());
    assert!(next.recovery.failed_backends().is_empty());
}

#[test]
fn unfinished_startup_without_an_exit_message_remains_recoverable() {
    let dir = tempfile::tempdir().unwrap();
    let mut guard = StartupGuard::open(dir.path()).unwrap();
    let pending = pending_attempt(Backend::Vulkan);
    guard.begin(pending.clone()).unwrap();
    let mut app = App::new();
    app.init_resource::<StartupPresentation>();
    install_guard(&mut app, guard);
    app.update();
    assert_eq!(StartupGuard::pending(dir.path()).unwrap(), Some(pending));
    drop(app);
    let next = StartupGuard::open(dir.path()).unwrap();
    assert_eq!(next.recovery.failed_backends(), [Backend::Vulkan]);
}

/// Delivers a renderer error through the installed policy and the application's exit systems.
fn restart_after_render_error(
    kind: bevy::render::error_handler::ErrorType,
    presented: bool,
    explicit: bool,
) -> launcher_host::renderer_startup::Recovery {
    use bevy::render::error_handler::{RenderError, RenderErrorHandler};

    let dir = tempfile::tempdir().unwrap();
    let mut guard = StartupGuard::open(dir.path()).unwrap();
    guard
        .begin(Attempt {
            explicit,
            ..pending_attempt(Backend::Dx12)
        })
        .unwrap();
    if presented {
        guard.presented().unwrap();
    }
    let mut app = App::new();
    app.init_resource::<StartupPresentation>();
    render::install_error_policy(&mut app);
    install_guard(&mut app, guard);
    crate::runtime::shutdown::TerminalCause::install(&mut app);
    app.add_systems(Update, crate::runtime::shutdown::exit_on_gpu_failure);
    let handler = app.world().resource::<RenderErrorHandler>().0;
    handler(
        &RenderError {
            ty: kind,
            description: "driver failure".into(),
            source: None,
        },
        app.world_mut(),
        &mut bevy::ecs::world::World::new(),
    );
    app.update();
    assert!(app.should_exit().is_some_and(|exit| exit.is_error()));
    drop(app);

    StartupGuard::open(dir.path()).unwrap().recovery
}

/// A completed first frame must not make a later lost device look like a healthy renderer.
#[test]
fn device_loss_after_presentation_uses_another_api_on_restart() {
    let recovery = restart_after_render_error(
        bevy::render::error_handler::ErrorType::DeviceLost,
        true,
        false,
    );
    assert_eq!(recovery.failed_backends(), [Backend::Dx12]);
    assert!(recovery.working.is_none());
    assert_eq!(
        candidates(
            Platform::Windows.backends(),
            Platform::Windows.backends(),
            None,
            None,
            recovery.failed_backends(),
        ),
        [Backend::Vulkan]
    );
}

#[test]
fn device_loss_before_presentation_survives_orderly_shutdown() {
    let recovery = restart_after_render_error(
        bevy::render::error_handler::ErrorType::DeviceLost,
        false,
        false,
    );
    assert_eq!(recovery.failed_backends(), [Backend::Dx12]);
    assert!(recovery.working.is_none());
}

#[test]
fn device_loss_under_a_saved_api_uses_the_existing_video_fallback() {
    let recovery = restart_after_render_error(
        bevy::render::error_handler::ErrorType::DeviceLost,
        true,
        true,
    );
    assert!(recovery.working.is_none());
    let (chosen, fallback) = with_video_fallback(
        false,
        Some(Backend::Dx12),
        recovery.crashed_choice(),
        Platform::Windows.backends(),
        |video, excluded| {
            assert_eq!(video, None);
            assert_eq!(excluded, [Backend::Dx12]);
            Ok(Backend::Vulkan)
        },
    )
    .unwrap();
    assert_eq!(chosen, Backend::Vulkan);
    assert_eq!(fallback.unwrap().requested, Backend::Dx12);
}

#[test]
fn out_of_memory_does_not_exclude_a_graphics_api_on_restart() {
    for presented in [false, true] {
        let recovery = restart_after_render_error(
            bevy::render::error_handler::ErrorType::OutOfMemory,
            presented,
            false,
        );
        assert!(recovery.failed_backends().is_empty());
        assert_eq!(recovery.crashed_choice(), None);
    }
}

#[test]
fn backend_mask_uses_the_policy_order() {
    assert_eq!(
        backends(wgpu::Backends::VULKAN | wgpu::Backends::DX12),
        [Backend::Vulkan, Backend::Dx12]
    );
    assert_eq!(backends(wgpu::Backends::DX12), [Backend::Dx12]);
    assert!(backends(wgpu::Backends::empty()).is_empty());
    assert!(backends(wgpu::Backends::GL).is_empty());
}

#[test]
fn fallback_notice_waits_for_a_visible_menu_and_opens_once() {
    let message = notice(
        &VideoFallback {
            requested: Backend::Dx12,
            reason: "device rejected".into(),
        },
        Backend::Vulkan,
        Platform::Windows,
    );
    let mut app = App::new();
    install_notice(&mut app, message.clone());
    app.update();
    app.insert_resource(client_shell::menu::MenuRuntime::new(
        false,
        2,
        String::new(),
    ));
    app.update();
    assert!(app.world().contains_resource::<FallbackNotice>());
    app.world_mut()
        .resource_mut::<client_shell::menu::MenuRuntime>()
        .set_visible(true);
    app.update();
    assert!(!app.world().contains_resource::<FallbackNotice>());
    let view = app
        .world()
        .resource::<client_shell::menu::MenuRuntime>()
        .view();
    assert_eq!(view.dialog, Some(launcher::menu::MenuDialog::Notice));
    assert_eq!(view.message.as_deref(), Some(message.as_str()));
    for expected in ["DX12", "Vulkan", "device rejected"] {
        assert!(message.contains(expected), "{message}");
    }
}

#[test]
fn fallback_notice_waits_for_an_open_dialog_to_close() {
    let mut app = App::new();
    install_notice(&mut app, "DX12 could not start".into());
    let mut menu = client_shell::menu::MenuRuntime::new(true, 2, String::new());
    menu.show_notice("Import your Minecraft settings?");
    app.insert_resource(menu);
    app.update();
    assert!(app.world().contains_resource::<FallbackNotice>());
    let menu = app.world().resource::<client_shell::menu::MenuRuntime>();
    assert_eq!(
        menu.view().message.as_deref(),
        Some("Import your Minecraft settings?")
    );
    app.world_mut()
        .resource_mut::<client_shell::menu::MenuRuntime>()
        .activate(launcher::menu::MenuAction::DismissDialog);
    app.update();
    assert!(!app.world().contains_resource::<FallbackNotice>());
    let view = app
        .world()
        .resource::<client_shell::menu::MenuRuntime>()
        .view();
    assert_eq!(view.dialog, Some(launcher::menu::MenuDialog::Notice));
    assert_eq!(view.message.as_deref(), Some("DX12 could not start"));
}
