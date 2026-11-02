//! Publishes camera continuity and saved interpolation preferences without changing game timing.
use bevy::{prelude::*, render::render_resource::TextureUsages};
use client_presentation::{
    camera::{
        CameraSettingsAuthority, FlyCamera, ServerCameraView, motion_blur::CameraContinuityKey,
    },
    local_player::LocalViewPose,
};
use client_state::{PlayerRuntime, RuntimeSettings};
use render_scene::frame_generation::FrameGenerationCamera;

/// Independent history for the optional effect, including transitions while motion blur is off.
#[derive(Default)]
pub(super) struct History {
    key: Option<CameraContinuityKey>,
    epoch: u64,
}

/// Attaches only to a ready SDR world; unsupported render effects retain ordinary presentation.
#[allow(clippy::too_many_arguments)] // Publishes existing authorities without sampling or advancing them.
pub(super) fn publish(
    mut commands: Commands,
    user: Res<RuntimeSettings>,
    #[cfg(feature = "developer-control")] recording: Option<
        Res<crate::developer_control::Recording>,
    >,
    player: Res<PlayerRuntime>,
    view: Res<LocalViewPose>,
    server: Res<ServerCameraView>,
    settings: Res<CameraSettingsAuthority>,
    pacing: Res<render::FramePacing>,
    mut history: Local<History>,
    display: Res<crate::present_mode::DisplayRefresh>,
    mut cameras: Query<
        (
            Entity,
            &mut Camera3d,
            &Projection,
            Option<&FrameGenerationCamera>,
        ),
        With<FlyCamera>,
    >,
) {
    let snapshot = user.user_settings_update();
    let video = &snapshot.1.video;
    #[cfg(feature = "developer-control")]
    let capture = recording.is_some();
    #[cfg(not(feature = "developer-control"))]
    let capture = false;
    let enabled = video.frame_generation
        && video.render_mode == ui::RenderMode::Vanilla
        && video.motion_blur == ui::MotionBlurQuality::Off
        && player.inventory.local_runtime_id().is_some();
    let key = CameraContinuityKey::current(&view, &server, &settings);
    if history.key != Some(key) {
        history.epoch = history.epoch.wrapping_add(1);
        history.key = Some(key);
    }
    for (entity, mut camera, projection, previous) in &mut cameras {
        let perspective = match projection {
            Projection::Perspective(projection) if enabled => Some(projection),
            _ => None,
        };
        let Some(projection) = perspective else {
            if previous.is_some() {
                commands.entity(entity).remove::<FrameGenerationCamera>();
            }
            continue;
        };
        let usages = TextureUsages::from(camera.depth_texture_usages);
        if !usages.contains(TextureUsages::TEXTURE_BINDING) {
            camera.depth_texture_usages = (usages | TextureUsages::TEXTURE_BINDING).into();
        }
        let desired = FrameGenerationCamera {
            reset_epoch: history.epoch,
            // Targets, never measured deltas: the presenter's own waits lengthen those.
            interval_ns: render_api::frame_generation::real_interval_ns(
                pacing.rate.map(|rate| rate.period_nanos()),
                display.0.refresh.map(|rate| rate.period_nanos()),
            ),
            capture,
            near: projection.near,
            far: f32::INFINITY,
            fov_y: projection.fov,
        };
        if previous != Some(&desired) {
            commands.entity(entity).insert(desired);
        }
    }
}
