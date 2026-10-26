//! The camera's published damage shake and its reproduction through a suppressing mod tilt.
use super::*;
use crate::camera::ViewTilt;
use crate::camera::actor_effects::DamageShake;
use semantic_input::PerspectiveMode;

/// The camera pose every application starts from.
fn base() -> Transform {
    Transform::from_xyz(3.0, 6.0, -8.0).with_rotation(Quat::from_euler(
        EulerRot::YXZ,
        0.7,
        -0.4,
        0.0,
    ))
}

/// Accepts `user` as the next settings generation.
fn configure(app: &mut App, generation: u64, user: &ui::UserSettings) {
    app.world_mut()
        .resource_mut::<CameraSettingsAuthority>()
        .replace(generation, user)
        .unwrap();
}

/// Applies the actor camera effects to the base pose under `tilt`, returning the published
/// shake, the presented camera and the hand's effect.
fn present_frame(
    app: &mut App,
    camera: Entity,
    tilt: Option<ViewTilt>,
) -> (Option<DamageShake>, Mat4, Option<Mat4>) {
    app.world_mut()
        .resource_mut::<CameraSettingsAuthority>()
        .set_view_tilt(tilt);
    *app.world_mut().get_mut::<Transform>(camera).unwrap() = base();
    app.world_mut().run_system_cached(apply).unwrap();
    (
        app.world().resource::<PresentedDamageShake>().0,
        app.world().get::<Transform>(camera).unwrap().to_matrix(),
        app.world()
            .resource::<FirstPersonHandMotion>()
            .legacy_effect,
    )
}

/// Completes `ticks` of the local player's actor clock.
fn advance(app: &mut App, ticks: u32) {
    app.world_mut()
        .resource_mut::<TestWorld>()
        .0
        .advance_actor_interpolation_frame(ticks);
}

/// Both hurt kinds publish the native shake at full strength, whatever the player's damage-shake
/// setting or a mod's suppression; nothing is published while idle, in third person or once the
/// shake ends.
#[test]
fn the_presented_damage_shake_is_the_full_strength_native_shake() {
    for kind in [ActorStatusKind::Hurt, ActorStatusKind::HurtWithoutDamage] {
        let mut app = camera_app();
        let camera = app.world_mut().spawn((FlyCamera::default(), base())).id();
        let mut user = ui::UserSettings::default();
        user.video.damage_bob = 0.25;
        configure(&mut app, 1, &user);
        assert_eq!(present_frame(&mut app, camera, None).0, None, "idle");
        event(&mut app.world_mut().resource_mut::<TestWorld>().0, 2, kind);
        advance(&mut app, 4);
        let angle = -render_model::legacy_combat::legacy_hurt_radians(6, 0.5)
            .unwrap()
            .to_degrees();
        assert!(angle > 1.0, "the sample is mid-shake");
        let expected = Some(DamageShake {
            roll_degrees: angle,
            pitch_degrees: 0.0,
        });
        assert_eq!(
            present_frame(&mut app, camera, None).0,
            expected,
            "{kind:?}"
        );
        let suppressing = ViewTilt {
            roll_degrees: 3.0,
            pitch_degrees: -2.0,
            suppress_hurt_shake: true,
        };
        assert_eq!(
            present_frame(&mut app, camera, Some(suppressing)).0,
            expected
        );
        user.gameplay.default_perspective = PerspectiveMode::ThirdPersonBack;
        configure(&mut app, 2, &user);
        assert_eq!(
            present_frame(&mut app, camera, None).0,
            None,
            "third person"
        );
        user.gameplay.default_perspective = PerspectiveMode::FirstPerson;
        configure(&mut app, 3, &user);
        assert_eq!(present_frame(&mut app, camera, None).0, expected);
        advance(&mut app, 6);
        assert_eq!(present_frame(&mut app, camera, None).0, None, "ended");
    }
}

/// At the player's full setting, a tilt of the presented shake that suppresses the native shake
/// presents the native camera and hand, view bob included, for both hurt kinds.
#[test]
fn a_suppressing_tilt_of_the_presented_shake_reproduces_the_native_camera() {
    for kind in [ActorStatusKind::Hurt, ActorStatusKind::HurtWithoutDamage] {
        for elapsed in [1, 4, 6] {
            let mut app = camera_app();
            for index in 0..4 {
                tick(&mut app, index);
            }
            let mut user = ui::UserSettings::default();
            user.video.damage_bob = 1.0;
            user.video.view_bobbing = ui::ViewBobbing::Full;
            configure(&mut app, 1, &user);
            app.world_mut().run_system_cached(sample).unwrap();
            event(&mut app.world_mut().resource_mut::<TestWorld>().0, 2, kind);
            advance(&mut app, elapsed);
            let camera = app.world_mut().spawn((FlyCamera::default(), base())).id();
            let (shake, native, native_hand) = present_frame(&mut app, camera, None);
            let shake = shake.unwrap();
            let tilt = |roll_degrees, pitch_degrees| ViewTilt {
                roll_degrees,
                pitch_degrees,
                suppress_hurt_shake: true,
            };
            let (_, tilted, tilted_hand) = present_frame(
                &mut app,
                camera,
                Some(tilt(shake.roll_degrees, shake.pitch_degrees)),
            );
            assert!(tilted.abs_diff_eq(native, 1e-5), "{kind:?} {elapsed}");
            assert!(tilted_hand.unwrap().abs_diff_eq(native_hand.unwrap(), 1e-5));
            let (_, still, _) = present_frame(&mut app, camera, Some(tilt(0.0, 0.0)));
            assert!(!still.abs_diff_eq(native, 1e-4), "the shake is visible");
        }
    }
}
