use super::*;

#[test]
fn aim_activation_reports_both_third_person_views_without_repeating_steady_frames() {
    let mut activation = AimActivation::default();
    let camera = ServerCameraView::default();
    let state = ServerAimAssist::default();
    let mut packets = Vec::new();
    for perspective in [
        PerspectiveMode::FirstPerson,
        PerspectiveMode::FirstPerson,
        PerspectiveMode::ThirdPersonBack,
        PerspectiveMode::ThirdPersonBack,
        PerspectiveMode::ThirdPersonFront,
        PerspectiveMode::ThirdPersonFront,
        PerspectiveMode::FirstPerson,
    ] {
        activation.publish(&camera, perspective, &state, Some((1, 0)), |packet| {
            packets.push(packet);
            true
        });
    }
    let expected = [false, true, true, false].map(|allow| {
        protocol::encode(&protocol::camera_aim_assist_activation_packet(
            "", allow, true,
        ))
        .unwrap()
    });
    assert_eq!(
        packets
            .iter()
            .map(|packet| protocol::encode(packet).unwrap())
            .collect::<Vec<_>>(),
        expected,
    );
}

#[test]
fn aim_activation_retries_a_rejected_perspective_change() {
    let mut activation = AimActivation::default();
    let camera = ServerCameraView::default();
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
