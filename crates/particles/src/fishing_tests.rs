use super::*;

fn direction(request: &SpawnRequest) -> [f32; 3] {
    ["direction.x", "direction.y", "direction.z"].map(|name| {
        request
            .variables
            .iter()
            .find(|(variable, _)| variable == name)
            .unwrap_or_else(|| panic!("{name} missing"))
            .1
    })
}

fn assert_close(actual: [f32; 3], expected: [f32; 3]) {
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert!((actual - expected).abs() < 1e-4, "{actual} != {expected}");
    }
}

#[test]
fn a_bite_sends_a_bubble_and_two_wakes_a_block_above_the_hook_floor() {
    let requests = fishing_bite_requests([4.25, 62.75, -3.5], 62.75, 0.25);
    assert_eq!(
        requests.each_ref().map(|request| request.native_effect),
        [
            Some(ParticleEffect::FishHookParticle),
            Some(ParticleEffect::WaterWakeParticle),
            Some(ParticleEffect::WaterWakeParticle),
        ]
    );
    for request in &requests {
        assert_eq!(request.position, [4.25, 63.0, -3.5]);
        assert_close(direction(request), [6.0, 0.0, 0.25]);
    }
}

#[test]
fn the_fish_trail_wakes_cross_the_approach_angle_beside_the_fish() {
    let angle = std::f32::consts::FRAC_PI_6;
    let (sin, cos) = angle.sin_cos();
    let mut bubbles = 0;
    for seed in 0..1000 {
        let requests = fish_trail_requests([4.25, 62.75, -3.5], 62.75, [1.5, -2.0], angle, seed);
        let (bubble, wakes) = requests.split_at(requests.len() - 2);
        for request in &requests {
            assert_close(request.position, [5.75, 62.9, -5.5]);
        }
        if let [bubble] = bubble {
            bubbles += 1;
            assert_eq!(bubble.native_effect, Some(ParticleEffect::FishPosParticle));
            assert_close(direction(bubble), [sin, 0.1, cos]);
        } else {
            assert!(bubble.is_empty());
        }
        assert!(
            wakes
                .iter()
                .all(|wake| wake.native_effect == Some(ParticleEffect::WaterWakeParticle))
        );
        assert_close(direction(&wakes[0]), [-cos * 0.004, 0.01, sin * 0.004]);
        assert_close(direction(&wakes[1]), [cos * 0.004, 0.01, -sin * 0.004]);
    }
    assert!(
        (100..200).contains(&bubbles),
        "{bubbles} bubbles in 1000 events"
    );
}

#[test]
fn a_tease_splashes_from_the_fish_cell_corner() {
    let cell = fish_tease_cell([4.25, 62.75, -3.5], 62.75, [1.5, -2.0]);
    assert_eq!(cell, Some([5, 62, -6]));
    let request = fish_tease_request([5, 62, -6]);
    assert_eq!(
        request.native_effect,
        Some(ParticleEffect::WaterSplashParticle)
    );
    assert_eq!(request.position, [5.0, 62.0, -6.0]);
    assert_close(direction(&request), [0.1; 3]);
    assert_eq!(
        fish_tease_cell([f32::NAN, 62.75, 0.0], 62.75, [0.0; 2]),
        None
    );
}
