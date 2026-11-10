use super::*;
use client_presentation::camera::ViewContext;

/// Installs a server camera registry the way a presets packet does.
fn with_presets(view: &mut ServerCameraView, names: &[(&str, &str)]) {
    let presets = names
        .iter()
        .map(|(name, inherit)| protocol::CameraPreset {
            name: Arc::from(*name),
            inherit_from: Arc::from(*inherit),
            ..Default::default()
        })
        .collect();
    view.apply(
        0,
        &protocol::CameraEvent::Presets(presets),
        &ViewContext {
            base: Transform::IDENTITY,
            subject: Transform::IDENTITY,
            base_fov: 70.0,
            actors: &|_| None,
        },
    );
}

fn vanilla_view() -> ServerCameraView {
    let mut view = ServerCameraView::default();
    with_presets(
        &mut view,
        &[
            ("minecraft:free", ""),
            ("minecraft:first_person", ""),
            ("minecraft:third_person", ""),
            ("minecraft:third_person_front", ""),
        ],
    );
    view
}

const CYCLE: [PerspectiveMode; 7] = [
    PerspectiveMode::FirstPerson,
    PerspectiveMode::FirstPerson,
    PerspectiveMode::ThirdPersonBack,
    PerspectiveMode::ThirdPersonBack,
    PerspectiveMode::ThirdPersonFront,
    PerspectiveMode::ThirdPersonFront,
    PerspectiveMode::FirstPerson,
];

fn encoded_cycle(activation: &mut AimActivation, camera: &ServerCameraView) -> Vec<Vec<u8>> {
    let state = ServerAimAssist::default();
    let mut packets = Vec::new();
    for perspective in CYCLE {
        activation.publish(camera, perspective, &state, Some((1, 0)), |packet| {
            packets.push(protocol::encode(&packet).unwrap().to_vec());
            true
        });
    }
    packets
}

#[test]
fn aim_activation_stays_silent_for_perspective_cameras_without_server_presets() {
    let mut activation = AimActivation::default();
    let mut camera = ServerCameraView::default();
    assert!(encoded_cycle(&mut activation, &camera).is_empty());
    with_presets(
        &mut camera,
        &[
            ("minecraft:free", ""),
            ("custom:first", "minecraft:first_person"),
            ("custom:third", "minecraft:third_person"),
        ],
    );
    assert!(encoded_cycle(&mut AimActivation::default(), &camera).is_empty());
}

#[test]
fn aim_activation_waits_for_the_next_activation_after_presets_arrive() {
    let mut activation = AimActivation::default();
    let state = ServerAimAssist::default();
    let mut camera = ServerCameraView::default();
    let mut packets = 0;
    activation.publish(
        &camera,
        PerspectiveMode::FirstPerson,
        &state,
        Some((1, 0)),
        |_| {
            packets += 1;
            true
        },
    );
    camera = vanilla_view();
    activation.publish(
        &camera,
        PerspectiveMode::FirstPerson,
        &state,
        Some((1, 0)),
        |_| {
            packets += 1;
            true
        },
    );
    assert_eq!(packets, 0);
    activation.publish(
        &camera,
        PerspectiveMode::ThirdPersonBack,
        &state,
        Some((1, 0)),
        |_| {
            packets += 1;
            true
        },
    );
    assert_eq!(packets, 1);
}

#[test]
fn aim_activation_reports_both_third_person_views_without_repeating_steady_frames() {
    let mut activation = AimActivation::default();
    let camera = vanilla_view();
    let expected = [false, true, true, false].map(|allow| {
        protocol::encode(&protocol::camera_aim_assist_activation_packet(
            "", allow, true,
        ))
        .unwrap()
        .to_vec()
    });
    assert_eq!(encoded_cycle(&mut activation, &camera), expected);
}

#[test]
fn aim_activation_retries_a_rejected_perspective_change() {
    let mut activation = AimActivation::default();
    let camera = vanilla_view();
    let state = ServerAimAssist::default();
    let identity = Some((1, 0));
    activation.publish(
        &camera,
        PerspectiveMode::ThirdPersonBack,
        &state,
        identity,
        |_| true,
    );
    let mut attempts = 0;
    activation.publish(
        &camera,
        PerspectiveMode::ThirdPersonFront,
        &state,
        identity,
        |_| {
            attempts += 1;
            false
        },
    );
    let mut packets = Vec::new();
    for _ in 0..2 {
        activation.publish(
            &camera,
            PerspectiveMode::ThirdPersonFront,
            &state,
            identity,
            |packet| {
                packets.push(packet);
                true
            },
        );
    }
    assert_eq!(attempts, 1);
    assert_eq!(packets.len(), 1);
    assert_eq!(
        protocol::encode(&packets[0]).unwrap(),
        protocol::encode(&protocol::camera_aim_assist_activation_packet(
            "", true, true
        ))
        .unwrap(),
    );
}
