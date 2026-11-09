//! A finger on the world aims interactions along its ray instead of the crosshair.
use super::*;
use crate::camera::freeform_pick::{FreeformPickPoint, publish};
use crate::ui_runtime::gameplay_touch::world_gestures::RAY_SETTLE_FRAMES;
use bevy::prelude::{PerspectiveProjection, Projection, Transform};

/// What the melee pick finds straight ahead of an actor after a finger at `point` has been
/// published for `frames` frames.
fn pick_after(point: Option<[f32; 2]>, frames: u8) -> Option<gameplay::melee::Crosshair> {
    let (mut world, _captured) = attack_hit_position_tests::unpublished_attack_fixture(7.5);
    attack_progress_tests::buttons(&mut world, vec![]);
    let forward = world
        .resource::<InteractionOriginSnapshot>()
        .outbound_ray()
        .unwrap()
        .direction();
    world.spawn((
        client_presentation::camera::FlyCamera::default(),
        Transform::default().looking_to(forward, Vec3::Y),
        Projection::Perspective(PerspectiveProjection {
            fov: 1.2,
            aspect_ratio: 16.0 / 9.0,
            ..Default::default()
        }),
    ));
    // The frame before the finger lands, which the first finger frame still reads.
    world
        .run_system_cached(crate::melee::prepare_melee)
        .unwrap();
    world.insert_resource(FreeformPickPoint(point));
    for _ in 0..frames {
        world.run_system_cached(publish).unwrap();
        world
            .run_system_cached(crate::melee::prepare_melee)
            .unwrap();
    }
    world.resource::<crate::melee::MeleeTarget>().crosshair
}

/// What the melee pick finds once a finger's ray has settled.
fn pick_with_finger(point: Option<[f32; 2]>) -> Option<gameplay::melee::Crosshair> {
    pick_after(point, RAY_SETTLE_FRAMES)
}

#[test]
fn a_finger_aims_the_pick_along_its_own_ray() {
    let actor = |pick| matches!(pick, Some(gameplay::melee::Crosshair::Actor(_)));
    assert!(
        actor(pick_with_finger(None)),
        "no finger keeps the crosshair ray"
    );
    assert!(
        actor(pick_with_finger(Some([0.5, 0.5]))),
        "the window centre is the crosshair ray"
    );
    let beside = pick_with_finger(Some([0.95, 0.5]));
    assert!(beside.is_some() && !actor(beside), "{beside:?}");
}

/// A tap must not act before this many frames, or it reads what the previous ray found.
#[test]
fn the_pick_reaches_a_finger_after_the_settle_frames() {
    let actor = |pick| matches!(pick, Some(gameplay::melee::Crosshair::Actor(_)));
    let beside = Some([0.95, 0.5]);
    assert!(actor(pick_after(beside, RAY_SETTLE_FRAMES - 1)));
    assert!(!actor(pick_after(beside, RAY_SETTLE_FRAMES)));
}
