//! Hands the player's HDR output request to the render world.
use bevy::{
    prelude::*,
    render::{Extract, ExtractSchedule, RenderApp},
};
use client_state::RuntimeSettings;

/// Extracts the saved HDR request each frame alongside the render-scale request.
pub(super) fn install(app: &mut App) {
    let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render_app.add_systems(ExtractSchedule, extract_hdr_request);
}

/// Requests the saved HDR output, or SDR defaults before settings exist; the resource is
/// replaced only when the request changes, so the renderer sees a change only on an edit.
fn extract_hdr_request(
    mut commands: Commands,
    settings: Extract<Option<Res<RuntimeSettings>>>,
    current: Option<Res<render::HdrOutputRequest>>,
) {
    let request = settings
        .as_ref()
        .map_or_else(render_api::hdr::HdrOutput::default, |settings| {
            settings.user_settings_update().1.video.hdr
        });
    if current.as_ref().map(|current| current.0) != Some(request) {
        commands.insert_resource(render::HdrOutputRequest(request));
    }
}
