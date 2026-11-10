use super::*;
use crate::render_mode::{GraphicsSettings, set_graphics_settings};

fn packs() -> PathBuf {
    PathBuf::from("data").join("shaderpacks")
}

fn cache() -> PathBuf {
    PathBuf::from("data").join("cache").join("shaderpacks")
}

fn config(launch: Option<PathBuf>) -> RenderModeConfig {
    RenderModeConfig {
        cli: None,
        attributable: false,
        path: None,
        cli_shader_pack: launch,
        shader_packs: Some(ShaderPackDirs {
            packs: packs(),
            cache: cache(),
            discovered: vec![("Nested".to_owned(), packs().join("Nested").join("Inner"))],
        }),
        shader_pack_revision: 0,
    }
}

fn app(config: RenderModeConfig) -> App {
    let mut app = App::new();
    app.init_resource::<RuntimeSettings>()
        .insert_resource(config)
        .init_resource::<ShaderPackRequest>()
        .add_systems(Update, sync_shader_pack_request);
    app
}

fn select(app: &mut App, mode: RenderMode, pack: Option<&str>) {
    set_graphics_settings(
        &mut app.world_mut().resource_mut::<RuntimeSettings>(),
        &GraphicsSettings {
            mode,
            shader_pack: pack.map(str::to_owned),
        },
    );
}

fn requested(app: &App) -> Option<PathBuf> {
    app.world().resource::<ShaderPackRequest>().pack.clone()
}

#[test]
fn request_follows_the_selected_pack_only_in_shader_pack_mode() {
    let mut app = app(config(None));
    app.update();
    assert_eq!(requested(&app), None);
    assert_eq!(
        app.world().resource::<ShaderPackRequest>().cache_dir,
        Some(cache())
    );
    select(&mut app, RenderMode::ShaderPack, Some("Dusk.zip"));
    app.update();
    assert_eq!(requested(&app), Some(packs().join("Dusk.zip")));
    select(&mut app, RenderMode::ShaderPack, Some("Nested"));
    app.update();
    assert_eq!(requested(&app), Some(packs().join("Nested").join("Inner")));
    select(&mut app, RenderMode::ShaderPack, Some("../escape"));
    app.update();
    assert_eq!(requested(&app), None);
    select(&mut app, RenderMode::ShaderPack, None);
    app.update();
    assert_eq!(requested(&app), None);
    select(&mut app, RenderMode::Vanilla, Some("Dusk.zip"));
    app.update();
    assert_eq!(requested(&app), None);
}

#[test]
fn the_launch_pack_wins_over_the_saved_selection() {
    let launch = PathBuf::from("elsewhere").join("Lumen");
    let mut app = app(config(Some(launch.clone())));
    select(&mut app, RenderMode::ShaderPack, Some("Dusk.zip"));
    app.update();
    assert_eq!(requested(&app), Some(launch));
    select(&mut app, RenderMode::Vanilla, Some("Dusk.zip"));
    app.update();
    assert_eq!(requested(&app), None);
}

#[test]
fn an_unchanged_request_is_not_rewritten() {
    let mut app = app(config(None));
    select(&mut app, RenderMode::ShaderPack, Some("Dusk.zip"));
    app.update();
    app.world_mut().clear_trackers();
    let same = app
        .world()
        .resource::<RuntimeSettings>()
        .user_settings_update()
        .1
        .clone();
    app.world_mut()
        .resource_mut::<RuntimeSettings>()
        .replace_user_settings(same);
    app.update();
    assert!(!app.world().resource_ref::<ShaderPackRequest>().is_changed());
}

#[test]
fn reselecting_a_failed_pack_requests_another_load() {
    use launcher::menu::{MenuAction, RendererChoice};
    use render_scene::shaderpack::ShaderPackStatus;

    let mut app = app(config(None));
    app.insert_resource(MenuRuntime::new(true, 2, "Steve".into()))
        .add_systems(
            Update,
            super::super::apply_menu_render_mode.before(sync_shader_pack_request),
        );
    select(&mut app, RenderMode::ShaderPack, Some("Dusk.zip"));
    app.update();
    let path = requested(&app).unwrap();
    let previous = app.world().resource::<ShaderPackRequest>().clone();
    app.insert_resource(ShaderPackStatus::Failed {
        pack: path.clone(),
        reason: "pack was missing".into(),
    });
    app.world_mut().clear_trackers();
    {
        let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
        menu.sync_shader_packs(vec!["Dusk.zip".to_owned()]);
        let revision = menu.view().shader_packs_revision;
        menu.activate(MenuAction::SelectRenderer(RendererChoice::ShaderPack(
            0, revision,
        )));
    }
    app.update();
    assert_eq!(requested(&app), Some(path));
    let reselected = app.world().resource::<ShaderPackRequest>().clone();
    assert_ne!(reselected, previous);
    app.world_mut().clear_trackers();
    app.update();
    assert_eq!(app.world().resource::<ShaderPackRequest>(), &reselected);
}

#[test]
fn a_menu_pick_replaces_the_launch_pack_and_saves_only_its_name() {
    use launcher::menu::{MenuAction, RendererChoice};

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("graphics.json");
    let launch = PathBuf::from("elsewhere").join("Lumen");
    let mut config = config(Some(launch.clone()));
    config.path = Some(path.clone());
    let mut app = app(config);
    app.insert_resource(MenuRuntime::new(true, 2, "Steve".into()))
        .add_systems(
            Update,
            super::super::apply_menu_render_mode.before(sync_shader_pack_request),
        );
    select(&mut app, RenderMode::ShaderPack, None);
    app.update();
    assert_eq!(requested(&app), Some(launch));
    {
        let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
        menu.sync_shader_packs(vec!["Dusk.zip".to_owned()]);
        let revision = menu.view().shader_packs_revision;
        menu.activate(MenuAction::SelectRenderer(RendererChoice::ShaderPack(
            0, revision,
        )));
    }
    app.update();
    assert_eq!(requested(&app), Some(packs().join("Dusk.zip")));
    let saved = super::super::load_graphics_settings(&path).unwrap();
    assert_eq!(saved.mode, RenderMode::ShaderPack);
    assert_eq!(saved.shader_pack.as_deref(), Some("Dusk.zip"));
    assert!(!std::fs::read_to_string(&path).unwrap().contains("Lumen"));
}

#[test]
fn only_opening_shaders_lists_packs_added_while_running() {
    use launcher::menu::{MenuAction, MenuScreen};

    let directory = tempfile::tempdir().unwrap();
    let mut config = config(None);
    config.shader_packs = Some(ShaderPackDirs {
        packs: directory.path().to_path_buf(),
        cache: cache(),
        discovered: Vec::new(),
    });
    let mut app = App::new();
    app.insert_resource(config)
        .insert_resource(MenuRuntime::new(true, 2, "Steve".into()))
        .add_message::<bevy::window::WindowFocused>()
        .add_systems(Update, refresh_shader_packs);
    app.update();
    std::fs::write(directory.path().join("Dusk.zip"), b"").unwrap();
    app.update();
    assert!(
        app.world()
            .resource::<MenuRuntime>()
            .view()
            .shader_packs
            .is_empty()
    );
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .activate(MenuAction::Navigate(MenuScreen::Settings));
    app.update();
    assert!(
        app.world()
            .resource::<MenuRuntime>()
            .view()
            .shader_packs
            .is_empty()
    );
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .activate(MenuAction::SettingsShaders);
    app.update();
    assert_eq!(
        app.world().resource::<MenuRuntime>().view().shader_packs[..],
        ["Dusk.zip".to_owned()]
    );
}

#[test]
fn regaining_focus_on_shaders_lists_packs_copied_in_meanwhile() {
    use launcher::menu::{MenuAction, MenuScreen};

    let directory = tempfile::tempdir().unwrap();
    let mut config = config(None);
    config.shader_packs = Some(ShaderPackDirs {
        packs: directory.path().to_path_buf(),
        cache: cache(),
        discovered: Vec::new(),
    });
    let mut app = App::new();
    app.insert_resource(config)
        .insert_resource(MenuRuntime::new(true, 2, "Steve".into()))
        .add_message::<bevy::window::WindowFocused>()
        .add_systems(Update, refresh_shader_packs);
    {
        let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
        menu.activate(MenuAction::Navigate(MenuScreen::Settings));
        menu.activate(MenuAction::SettingsShaders);
    }
    app.update();
    std::fs::write(directory.path().join("Dusk.zip"), b"").unwrap();
    let focus = |focused| bevy::window::WindowFocused {
        window: Entity::PLACEHOLDER,
        focused,
    };
    app.world_mut().write_message(focus(false));
    app.update();
    assert!(
        app.world()
            .resource::<MenuRuntime>()
            .view()
            .shader_packs
            .is_empty(),
        "losing focus leaves the folder unscanned"
    );
    app.world_mut().write_message(focus(true));
    app.update();
    assert_eq!(
        app.world().resource::<MenuRuntime>().view().shader_packs[..],
        ["Dusk.zip".to_owned()]
    );
}

/// Counts actual executions of the production shader-settings schedule.
fn frame_executions(app: &mut App) -> usize {
    app.world_mut().schedule_scope(Update, |world, schedule| {
        schedule.initialize(world).unwrap();
        let before = schedule
            .systems()
            .unwrap()
            .map(|(id, system)| (id, system.get_last_run()))
            .collect::<std::collections::HashMap<_, _>>();
        schedule.run(world);
        schedule
            .systems()
            .unwrap()
            .filter(|(id, system)| {
                system.system_type() != std::any::TypeId::of::<bevy::ecs::schedule::ApplyDeferred>()
                    && before[id] != system.get_last_run()
            })
            .count()
    })
}

/// Installs every production shader-settings system with isolated, unused settings.
fn scheduled_app() -> App {
    let mut app = App::new();
    app.insert_resource(config(None))
        .init_resource::<RuntimeSettings>()
        .init_resource::<ShaderPackRequest>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<Time<bevy::time::Real>>()
        .add_message::<bevy::window::FileDragAndDrop>();
    configure(&mut app);
    app
}

#[test]
fn idle_settings_execute_no_shader_pack_systems() {
    let mut app = scheduled_app();
    app.init_resource::<render_scene::shaderpack::ShaderPackStatus>()
        .add_plugins(crate::shader_pack_inputs::ShaderPackInputsPlugin);
    assert_eq!(frame_executions(&mut app), 0);
}

#[test]
fn selecting_then_disabling_a_pack_returns_settings_to_zero_systems() {
    let mut app = scheduled_app();
    select(&mut app, RenderMode::ShaderPack, Some("Dusk.zip"));
    assert!(frame_executions(&mut app) > 0);
    assert!(requested(&app).is_some());
    select(&mut app, RenderMode::Vanilla, None);
    assert!(frame_executions(&mut app) > 0);
    assert!(requested(&app).is_none());
    assert_eq!(frame_executions(&mut app), 0);
}

#[test]
fn off_withdraws_loading_feedback_before_settings_become_idle() {
    use client_shell::ui_resources::UiRuntimeResource;
    use render_scene::shaderpack::ShaderPackStatus;
    let mut app = scheduled_app();
    app.insert_resource(MenuRuntime::new(false, 2, "Test".into()))
        .insert_resource(UiRuntimeResource(client_ui::ui_runtime::UiRuntime::new(1)))
        .insert_resource(ShaderPackStatus::Loading(packs().join("Dusk.zip")));
    select(&mut app, RenderMode::ShaderPack, Some("Dusk.zip"));
    frame_executions(&mut app);
    app.world_mut()
        .resource_mut::<Time<bevy::time::Real>>()
        .advance_by(launcher::menu::shader_pack_status::LOADING_TOAST_DELAY);
    frame_executions(&mut app);
    assert_eq!(
        app.world()
            .resource::<UiRuntimeResource>()
            .hud()
            .standing_toast()
            .unwrap()
            .until_millis,
        u64::MAX
    );
    select(&mut app, RenderMode::Vanilla, None);
    frame_executions(&mut app);
    app.world_mut().insert_resource(ShaderPackStatus::Idle);
    assert!(frame_executions(&mut app) > 0);
    assert_ne!(
        app.world()
            .resource::<UiRuntimeResource>()
            .hud()
            .standing_toast()
            .unwrap()
            .until_millis,
        u64::MAX
    );
    assert_eq!(frame_executions(&mut app), 0);
}
