use bevy::{
    prelude::{Local, Query, Res, ResMut, With},
    window::{PrimaryWindow, Window},
};
use client_shell::menu::MenuRuntime;
use client_shell::ui_resources::{UiPresentationResource, UiRuntimeResource};
use launcher::menu::settings_options::SettingId;
use render_model::panorama::{launcher_faces, launcher_view, overlay_tint};
use render_scene::PanoramaScene;
use std::{sync::Arc, time::Instant};
/// Uploads each carrier once and keeps its GPU faces ready for returning to the menu.
/// Uploaded CPU pixels are released; in-world Settings retains the game beneath it.
#[allow(
    clippy::too_many_arguments,
    reason = "Bevy system parameters: the screen state the panorama follows"
)]
pub(crate) fn drive_menu_panorama(
    player_runtime: bevy::prelude::Res<client_state::PlayerRuntime>,
    presentation: Res<UiPresentationResource>,
    runtime: Option<Res<UiRuntimeResource>>,
    menu: Option<Res<MenuRuntime>>,
    scene: Option<ResMut<PanoramaScene>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut state: Local<Option<PanoramaState>>,
) {
    let Some(mut scene) = scene else {
        return;
    };
    let leave = bridge::leave_profile::LeaveProfile::pending_presentation();
    let _section = leave.section_once("menu_background");
    #[cfg(feature = "tracy")]
    let _span = bevy::log::info_span!("menu.background").entered();
    // The renderer keeps the faces on the GPU; their pixels are only needed until then.
    scene.release_uploaded_faces();
    let renders_game = client_shell::screen_policy::renders_game(
        &player_runtime,
        runtime.as_deref().map(|runtime| &runtime.0),
        menu.as_deref(),
        Some(&presentation),
    );
    scene.set_game_visible(renders_game);
    let Some(assets) = presentation.ui_assets() else {
        scene.show(None);
        return;
    };
    if state.as_ref().is_none_or(|state| !state.is_source(assets)) {
        let seconds = state.as_ref().map_or(0.0, |state| state.seconds);
        let previous = state.replace(PanoramaState {
            source: assets.clone(),
            last: Instant::now(),
            seconds,
            tint: overlay_tint(assets),
        });
        if let Some(previous) = previous {
            client_runtime::session::release_off_frame(previous);
        }
        scene.set_faces(launcher_faces(assets).map(Arc::new));
    }
    let shown = presentation
        .loading_stage()
        .is_some_and(ui::LoadingStage::is_world_entry)
        || menu.as_deref().is_some_and(MenuRuntime::uses_panorama);
    let aspect = windows
        .iter()
        .next()
        .map(|window| window.width() / window.height().max(1.0))
        .unwrap_or(16.0 / 9.0);
    let Some(state) = state.as_mut() else {
        return;
    };
    let now = Instant::now();
    let speed = menu.as_ref().map_or(1.0, |menu| {
        menu.settings_snapshot().0.value(SettingId::PanoramaSpeed) as f32 / 100.0
    });
    state.seconds += now.duration_since(state.last).as_secs_f32() * speed;
    state.last = now;
    let has_faces = scene.has_faces();
    scene.show((shown && has_faces).then(|| launcher_view(state.seconds, aspect, state.tint)));
}

/// Retains the carrier owner so its shared file identity cannot be reused while cached.
pub(crate) struct PanoramaState {
    source: assets::RuntimeUiAssets,
    last: Instant,
    seconds: f32,
    tint: [f32; 4],
}

impl PanoramaState {
    /// Cloned views share the same file backing; a replacement carrier invalidates the faces.
    fn is_source(&self, assets: &assets::RuntimeUiAssets) -> bool {
        std::ptr::eq(self.source.ui_files(), assets.ui_files())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assets::{RuntimeUiAssets, UiAtlasPage, UiFile};
    use bevy::prelude::*;
    use client_ui::ui_runtime::presentation::UiPresentationRuntime;

    /// Builds an original one-pixel panorama without installed game carriers or file overrides.
    fn panorama_presentation() -> UiPresentationRuntime {
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(1, 1, image::Rgba([17, 42, 91, 255]))
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let mut files = vec![
            UiFile {
                path: "ui/_global_variables.json".into(),
                bytes: b"{}".as_slice().into(),
            },
            UiFile {
                path: "ui/_ui_defs.json".into(),
                bytes: br#"{"ui_defs":[]}"#.as_slice().into(),
            },
        ];
        files.extend((0..6).map(|index| UiFile {
            path: format!("textures/ui/panorama_{index}.png").into(),
            bytes: png.get_ref().clone().into(),
        }));
        files.sort_by(|left, right| left.path.cmp(&right.path));
        let page = UiAtlasPage {
            width: 1,
            height: 1,
            rgba8: vec![255; 4].into(),
        };
        let bytes = assets::encode_ui_catalog([1; 32], &[page], &[], &[], &files).unwrap();
        let mut presentation =
            UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
        presentation
            .enable_json_ui(Arc::new(RuntimeUiAssets::decode(&bytes).unwrap()))
            .unwrap();
        presentation
    }

    /// A menu-world-menu round trip must reuse its warm GPU faces and release CPU pixels.
    #[test]
    fn leaving_reuses_the_uploaded_menu_panorama_without_decoding_or_uploading_faces() {
        let mut app = App::new();
        let mut menu = MenuRuntime::new(true, 2, "Player".into());
        client_shell::menu::test_support::suppress_catalog(&mut menu);
        app.insert_resource(menu)
            .insert_resource(client_state::PlayerRuntime(player_state::PlayerState::new(
                1,
            )))
            .insert_resource(UiPresentationResource(panorama_presentation()))
            .init_resource::<PanoramaScene>()
            .add_systems(Update, drive_menu_panorama);
        app.update();
        let warm = app.world().resource::<PanoramaScene>().faces_revision;
        app.world().resource::<PanoramaScene>().mark_uploaded(warm);
        app.world_mut().resource_mut::<MenuRuntime>().show_world();
        app.update();
        assert!(app.world().resource::<PanoramaScene>().game_visible());
        assert!(
            app.world().resource::<PanoramaScene>().faces.is_none(),
            "uploaded CPU pixels should be released"
        );
        app.world_mut()
            .resource_mut::<MenuRuntime>()
            .show_after_disconnect();
        app.update();
        let scene = app.world().resource::<PanoramaScene>();
        assert!(scene.view.is_some());
        assert_eq!(
            scene.faces_revision, warm,
            "leaving must not invalidate the warm texture or request another upload"
        );
        assert!(
            scene.faces.is_none(),
            "leaving must not decode the faces again"
        );
    }
}
