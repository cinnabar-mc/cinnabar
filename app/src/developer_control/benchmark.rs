//! Read-only observations consumed by the scripted resident-render benchmark.

use bevy::{prelude::*, window::PrimaryWindow};
use serde_json::{Value, json};

/// Returns completed startup endpoints and the settings actually consumed by this session.
pub(super) fn snapshot(world: &mut World) -> Result<Value, String> {
    let mut windows = world.query_filtered::<&Window, With<PrimaryWindow>>();
    let window = windows.single(world).map_err(|_| "no primary window")?;
    let window = json!({
        "physical": [window.physical_width(), window.physical_height()],
        "logical": [window.width(), window.height()], "scale": window.scale_factor(),
        "hidden": !window.visible, "present_mode": format!("{:?}", window.present_mode),
    });
    let mut cameras = world
        .query_filtered::<(&Camera, &Projection), With<client_presentation::camera::FlyCamera>>();
    let camera = cameras.single(world).ok().map(|(camera, projection)| {
        json!({
            "viewport": camera.physical_viewport_size().map(|size| size.to_array()),
            "projection": format!("{projection:?}"),
        })
    });
    let settings = world
        .get_resource::<client_state::RuntimeSettings>()
        .ok_or("settings unavailable")?
        .user_settings_update()
        .1;
    let video = json!({
        "native_video_options": format!("{:?}", settings.video),
        "render_distance": settings.video.render_distance_chunks,
        "render_distance_blocks": usize::from(settings.video.render_distance_chunks) * world::SUB_CHUNK_SIDE,
        "render_scale_percent": settings.video.render_scale_percent,
    });
    let pacing = world.get_resource::<render::FramePacing>().map(|pacing| {
        json!({
            "uncapped": pacing.rate.is_none(), "display_sync": pacing.display_sync,
        })
    });
    let graphics = world.get_resource::<render_scene::VisibilityDiagnostics>()
        .and_then(|diagnostics| diagnostics.graphics_adapter())
        .map(|adapter| json!({
            "name": adapter.adapter, "backend": adapter.backend, "driver": adapter.driver,
            "driver_info": adapter.driver_info, "effective_present_mode": adapter.effective_present_mode,
            "present_mode_proven": adapter.present_mode_proven,
        }));
    let title =
        render_scene::startup_timing::current().and_then(|timing| timing.completed_menu_ms());
    let completion = bridge::join_profile::JoinProfile::current().completion();
    let in_world = completion
        .filter(|(stage, _)| *stage == "first_world_frame_completed")
        .map(|(_, at)| {
            at.saturating_duration_since(crate::lifecycle::launched_at())
                .as_secs_f64()
                * 1000.
        });
    let distance = world
        .get_resource::<crate::runtime::world::ClientWorld>()
        .and_then(|client| client.stream.as_ref())
        .map(|stream| stream.render_distance_blocks());
    Ok(json!({
        "app": {"name": "cinnabar", "version": env!("CARGO_PKG_VERSION"), "commit": diagnostics::session::COMMIT},
        "build_profile": diagnostics::session::BUILD_PROFILE,
        "game_version": protocol::GAME_VERSION, "protocol_version": protocol::PROTOCOL_VERSION,
        "window": window, "camera": camera, "video": video, "pacing": pacing, "gpu": graphics,
        "render_distance_blocks": distance, "title_ms": title, "world_ms": in_world,
        "join_outcome": completion.map(|(stage, _)| stage),
        "startup_origin": "process_entry", "world_endpoint": "loading_free_gpu_completed",
    }))
}
