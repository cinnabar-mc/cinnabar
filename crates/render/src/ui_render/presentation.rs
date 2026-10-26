//! Complete UI output for this render frame, consumed before GPU completion is registered.

use super::composite::UiLayerTexture;
use bevy::{
    prelude::{ContainsEntity, Query, ResMut, Resource, With},
    render::{camera::ExtractedCamera, sync_world::MainEntity},
    window::PrimaryWindow,
};

/// Distinguishes real interface output from a skipped, empty or clear-only frame.
#[derive(Resource, Default)]
pub struct UiFrameContent {
    window: bool,
    offscreen: bool,
}

impl UiFrameContent {
    /// Whether this frame composited complete UI to the requested kind of output.
    pub fn complete(&self, hidden: bool) -> bool {
        if hidden { self.offscreen } else { self.window }
    }
}

/// Reads the compositor's actual per-frame result after all render graph nodes have run.
pub(super) fn observe_ui_frame(
    layers: Query<(&ExtractedCamera, &UiLayerTexture)>,
    primary: Query<&MainEntity, With<PrimaryWindow>>,
    mut content: ResMut<UiFrameContent>,
) {
    content.window = false;
    content.offscreen = false;
    let primary = primary.single().ok().map(|entity| entity.id());
    for (camera, layer) in &layers {
        if !layer.presented_complete_content() {
            continue;
        }
        match &camera.target {
            Some(bevy::camera::NormalizedRenderTarget::Window(window))
                if Some(window.entity()) == primary =>
            {
                content.window = true
            }
            Some(bevy::camera::NormalizedRenderTarget::Image(_)) => content.offscreen = true,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::prelude::{App, Update};

    /// A later empty frame must not reuse a previous frame's completed interface evidence.
    #[test]
    fn an_empty_render_frame_clears_previous_interface_completion() {
        let mut app = App::new();
        app.insert_resource(UiFrameContent {
            window: true,
            offscreen: true,
        })
        .add_systems(Update, observe_ui_frame);
        app.update();
        let content = app.world().resource::<UiFrameContent>();
        assert!(!content.complete(false));
        assert!(!content.complete(true));
    }
}
