use bevy::prelude::*;
use client_audio::audio_ingress::SequencedAudioEvent;
use client_audio::named_audio::{AudioDevice, NamedAudio};
use client_presentation::local_player_camera_receipt::CameraPublicationAttempt;
use client_shell::ui_resources::UiRuntimeResource;
use launcher::menu::audio_output::{
    HELP_URL, OutageNotice, UNAVAILABLE_MESSAGE, UNAVAILABLE_TITLE,
};
use std::sync::Arc;
use {
    crate::runtime::world::ClientWorld,
    client_presentation::environment::WorldClock,
    client_presentation::{
        camera::FlyCamera,
        local_player::{CameraPose, LocalPlayerFrameCarrier},
    },
};

/// Borrows current owner facts and forwards them at the existing system boundary.
#[allow(clippy::too_many_arguments)]
pub(crate) fn drain_live_named_audio(
    messages: MessageReader<SequencedAudioEvent>,
    world: Res<ClientWorld>,
    clock: Res<WorldClock>,
    receipt: Res<CameraPublicationAttempt>,
    frame: Res<LocalPlayerFrameCarrier>,
    camera_pose: Res<CameraPose>,
    physics: Res<client_interaction::movement::LocalPhysicsController>,
    cameras: Query<&Transform, With<FlyCamera>>,
    state: ResMut<NamedAudio>,
    device: Option<NonSendMut<AudioDevice>>,
    profiler: Option<Res<render_scene::RuntimeStageProfiler>>,
) {
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(render_scene::RuntimeStage::Audio));
    let camera = client_presentation::local_player_camera_receipt::published_default_camera(
        client_presentation::observations::WorldObservation {
            stream: world.stream.as_ref(),
        },
        simulation_clock::identity::SessionGeneration::new(clock.session_generation()),
        &receipt,
        &frame,
        &camera_pose,
        &*physics,
        &cameras,
    );
    client_audio::named_audio::drain_live_named_audio(
        messages,
        client_audio::observations::WorldObservation {
            stream: world.stream.as_ref(),
        },
        client_audio::observations::SessionObservation(clock.session_generation()),
        camera,
        state,
        device,
    );
}

/// Raises the "No audio" toast once each time the sound output stays unopenable, in menus
/// and in game, and logs where the step-by-step fixes are. `now` is the real time since
/// startup that stamps the toast; `ui` is only written, and so only marked changed, when the
/// toast is raised.
pub(crate) fn notify_audio_output_unavailable(
    device: &mut AudioDevice,
    mut ui: ResMut<UiRuntimeResource>,
    now: std::time::Duration,
    notice: &mut OutageNotice,
) {
    if !notice.observe(device.output_unavailable()) {
        return;
    }
    warn!(
        help = HELP_URL,
        "sound output still unavailable; showing the No audio toast"
    );
    let now_millis = u64::try_from(now.as_millis()).unwrap_or(u64::MAX);
    ui.0.push_client_toast(
        Arc::from(UNAVAILABLE_TITLE),
        Arc::from(UNAVAILABLE_MESSAGE),
        now_millis,
    );
}

#[cfg(test)]
#[path = "named_audio/composed_tests.rs"]
mod composed_tests;
