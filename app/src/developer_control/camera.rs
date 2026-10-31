//! The scripted cinematic camera: overrides the rendered camera after every player and
//! server camera writer, leaving the player's own view and physics untouched.

use bevy::prelude::*;
use developer_control::camera::CameraPath;
use serde_json::{Value, json};

use {
    crate::runtime::telemetry::bedrock_camera_rotation, client_presentation::camera::FlyCamera,
    client_state::ClientFrameSet,
};

#[derive(Resource)]
pub(super) struct ScriptedCamera {
    path: CameraPath,
    /// Selected clock at the first sampled frame.
    started: Option<f64>,
    elapsed: f32,
    realtime: bool,
    /// The hand is hidden by an in-memory override, lifted on release.
    hid_hand: bool,
}

/// The vanilla video option the cinematic camera borrows to hide the hand.
const HIDE_HAND: launcher::menu::settings_options::SettingId =
    launcher::menu::settings_options::SettingId::HideHand;

impl ScriptedCamera {
    pub(super) fn finished(&self) -> bool {
        self.path.finished(self.elapsed)
    }

    pub(super) fn summary(&self) -> Value {
        json!({
            "elapsed": self.elapsed,
            "duration": self.path.duration(),
            "looping": self.path.looping,
            "finished": self.finished(),
        })
    }
}

pub(super) fn configure(app: &mut App) {
    app.add_systems(
        Update,
        (
            drive_camera
                .after(ClientFrameSet::Camera)
                .before(client_presentation::camera::motion_blur::apply_camera_motion_blur)
                .before(ClientFrameSet::Interaction),
            drive_camera_fov
                .after(drive_camera)
                .after(client_presentation::camera::update_camera_fov)
                .before(client_presentation::camera::overlay_publish::publish_screen_overlays)
                .before(ClientFrameSet::UiPreparation),
            drive_render_pose
                .after(client_presentation::camera::actor_effects::ActorCameraEffects)
                .after(drive_camera)
                .before(client_presentation::camera::overlay_publish::publish_screen_overlays)
                .before(ClientFrameSet::UiPreparation),
        ),
    );
}

pub(super) fn start(world: &mut World, path: CameraPath) -> Result<Value, String> {
    start_with_clock(world, path, false)
}

/// Starts a camera-only benchmark route using unclamped real elapsed time.
pub(super) fn start_realtime(world: &mut World, path: CameraPath) -> Result<Value, String> {
    start_with_clock(world, path, true)
}

/// Shares camera ownership and cleanup between game-clock captures and real-time routes.
fn start_with_clock(world: &mut World, path: CameraPath, realtime: bool) -> Result<Value, String> {
    path.validate()?;
    let duration = path.duration();
    let previously_hid = world
        .remove_resource::<ScriptedCamera>()
        .is_some_and(|previous| previous.hid_hand);
    let hid_hand = path.hide_hand;
    if previously_hid != hid_hand {
        set_hand_hidden(world, hid_hand);
    }
    if let Some(mut view) =
        world.get_resource_mut::<client_presentation::local_player::LocalViewPose>()
    {
        view.reanchor_camera();
    }
    world.insert_resource(ScriptedCamera {
        path,
        started: None,
        elapsed: 0.0,
        realtime,
        hid_hand,
    });
    Ok(json!({ "duration": duration }))
}

pub(super) fn release(world: &mut World) -> Result<Value, String> {
    let Some(scripted) = world.remove_resource::<ScriptedCamera>() else {
        return Ok(json!({ "released": false }));
    };
    if let Some(mut view) =
        world.get_resource_mut::<client_presentation::local_player::LocalViewPose>()
    {
        view.reanchor_camera();
    }
    if scripted.hid_hand {
        set_hand_hidden(world, false);
    }
    Ok(json!({ "released": true }))
}

/// Never saved: the override lives only in this process's settings snapshot.
fn set_hand_hidden(world: &mut World, hidden: bool) {
    if let Some(mut menu) = world.get_resource_mut::<client_shell::menu::MenuRuntime>() {
        menu.set_session_option(HIDE_HAND, hidden.then_some(1));
    }
}

/// Preserves game-clock recordings and uses unclamped real time only for explicit benchmark paths.
fn drive_camera(
    time: Res<Time>,
    real: Option<Res<Time<Real>>>,
    profiler: Option<Res<render_scene::RuntimeStageProfiler>>,
    scripted: Option<ResMut<ScriptedCamera>>,
    view: Option<ResMut<client_presentation::local_player::LocalViewPose>>,
    mut cameras: Query<&mut Transform, With<FlyCamera>>,
) {
    let Some(mut scripted) = scripted else {
        return;
    };
    let now = if scripted.realtime {
        let Some(real) = real else {
            return;
        };
        real.elapsed_secs_f64()
    } else {
        f64::from(time.elapsed_secs())
    };
    if scripted.started.is_none()
        && scripted.realtime
        && let Some(profiler) = profiler
    {
        profiler.trace_marker("benchmark_path_start");
    }
    let started = *scripted.started.get_or_insert(now);
    let previous = scripted.elapsed;
    scripted.elapsed = (now - started) as f32;
    let Some(pose) = sampled_pose(&scripted) else {
        return;
    };
    if scripted.path.crossed_cut(previous, scripted.elapsed)
        && let Some(mut view) = view
    {
        view.reanchor_camera();
    }
    for mut transform in &mut cameras {
        *transform = pose;
    }
}

/// Converts the current cinematic sample to its complete rendered pose.
fn sampled_pose(scripted: &ScriptedCamera) -> Option<Transform> {
    let sample = scripted.path.sample(scripted.elapsed)?;
    Some(
        Transform::from_translation(Vec3::from_array(sample.position))
            .with_rotation(bedrock_camera_rotation(sample.yaw, sample.pitch)),
    )
}

/// Preserves the sampled capture pose after actor-clock camera effects finish.
fn drive_render_pose(
    scripted: Option<Res<ScriptedCamera>>,
    mut cameras: Query<&mut Transform, With<FlyCamera>>,
) {
    let Some(pose) = scripted.as_deref().and_then(sampled_pose) else {
        return;
    };
    for mut camera in &mut cameras {
        *camera = pose;
    }
}

/// Applies the sampled FOV after gameplay projection updates, preserving portal distortion.
fn drive_camera_fov(
    scripted: Option<Res<ScriptedCamera>>,
    mut cameras: Query<&mut Projection, With<FlyCamera>>,
) {
    let Some(fov) = scripted
        .as_ref()
        .and_then(|scripted| scripted.path.sample(scripted.elapsed))
        .and_then(|sample| sample.fov)
    else {
        return;
    };
    for mut projection in &mut cameras {
        client_presentation::camera::set_camera_projection_fov(&mut projection, fov);
    }
}

#[cfg(test)]
mod tests;
