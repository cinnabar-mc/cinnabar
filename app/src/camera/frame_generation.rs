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
    time: Res<Time<Real>>,
    pacing: Res<render::FramePacing>,
    mut history: Local<History>,
    display: Res<crate::present_mode::DisplayRefresh>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
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
        let Projection::Perspective(projection) = projection else {
            commands.entity(entity).remove::<FrameGenerationCamera>();
            continue;
        };
        if !enabled {
            commands.entity(entity).remove::<FrameGenerationCamera>();
            continue;
        }
        camera.depth_texture_usages = (TextureUsages::from(camera.depth_texture_usages)
            | TextureUsages::TEXTURE_BINDING)
            .into();
        let desired = FrameGenerationCamera {
            reset_epoch: history.epoch,
            interval_ns: u64::try_from(time.delta().as_nanos())
                .unwrap_or(u64::MAX)
                .max(pacing.rate.map_or(0, |rate| rate.period_nanos())),
            capture,
            near: projection.near,
            far: f32::INFINITY,
            fov_y: projection.fov,
            synchronized_refresh_ns: windows
                .single()
                .ok()
                .filter(|window| {
                    matches!(
                        window.present_mode,
                        bevy::window::PresentMode::Fifo
                            | bevy::window::PresentMode::FifoRelaxed
                            | bevy::window::PresentMode::AutoVsync
                    )
                })
                .map(|_| display.0.refresh.map_or(0, |rate| rate.period_nanos())),
        };
        if previous != Some(&desired) {
            commands.entity(entity).insert(desired);
        }
    }
}
