use super::*;

#[test]
fn hand_bob_multiplier_keeps_camera_bob_and_hurt_in_both_animation_modes() {
    for java in [false, true] {
        let mut app = camera_app();
        let mut user = ui::UserSettings::default();
        user.video.java_animations = java;
        user.video.view_bobbing = ui::ViewBobbing::Full;
        app.world_mut()
            .resource_mut::<CameraSettingsAuthority>()
            .replace(1, &user)
            .unwrap();
        for index in 0..4 {
            tick(&mut app, index);
        }
        app.world_mut().run_system_cached(sample).unwrap();
        event(
            &mut app.world_mut().resource_mut::<TestWorld>().0,
            2,
            ActorStatusKind::Hurt,
        );
        app.world_mut()
            .resource_mut::<TestWorld>()
            .0
            .advance_actor_interpolation_frame(5);
        let endpoints = app.world().resource::<LegacyCameraMotion>().state.endpoints;
        let hurt = render_model::legacy_combat::legacy_hurt_matrix(5, 0.0, None, 0.5, 1.0);
        let camera = app
            .world_mut()
            .spawn((FlyCamera::default(), Transform::IDENTITY))
            .id();
        let mut native_camera = None;
        for strength in [1.0, 0.0, 0.5] {
            app.world_mut()
                .resource_mut::<CameraSettingsAuthority>()
                .set_hand_motion(Some(crate::camera::HandMotion {
                    bob: strength,
                    ..Default::default()
                }));
            app.world_mut().run_system_cached(sample).unwrap();
            *app.world_mut().get_mut::<Transform>(camera).unwrap() = Transform::IDENTITY;
            app.world_mut().run_system_cached(apply).unwrap();
            let hand = app
                .world()
                .resource::<FirstPersonHandMotion>()
                .legacy_effect
                .unwrap();
            let scaled = render_model::legacy_combat::LegacyCameraEndpoints {
                bob: endpoints.bob.map(|value| value * strength),
                fall_degrees: endpoints.fall_degrees.map(|value| value * strength),
                ..endpoints
            };
            assert!(hand.abs_diff_eq(hurt * scaled.bob_matrix(0.5), 3e-6));
            if strength == 0.0 {
                assert_ne!(hurt, Mat4::IDENTITY);
                assert!(!hand.abs_diff_eq(hurt * endpoints.bob_matrix(0.5), 1e-5));
            }
            let rendered = app.world().get::<Transform>(camera).unwrap().to_matrix();
            assert_eq!(rendered, *native_camera.get_or_insert(rendered));
        }
    }
}
