use super::*;

const IDENTITY: [[f32; 4]; 3] = [
    [1.0, 0.0, 0.0, 5.0],
    [0.0, 1.0, 0.0, 6.0],
    [0.0, 0.0, 1.0, 7.0],
];

#[test]
fn alive_is_untouched_and_finished_death_lies_on_its_side() {
    assert_eq!(death_tilted(IDENTITY, None), IDENTITY);
    let lying = death_tilted(IDENTITY, Some(1.0));
    // The local up axis now points along world +/-X while the feet pivot stays fixed.
    assert!(lying[0][1].abs() > 0.999 && lying[1][1].abs() < 1e-6);
    assert_eq!([lying[0][3], lying[1][3], lying[2][3]], [5.0, 6.0, 7.0]);
}
#[test]
fn review_render_local_placement_keeps_axis_scaling_and_death_tilt() {
    let old_feet = [1.0, 2.0, 3.0];
    let new_feet = [4.0, 64.0, 2.0];
    let axes = [2.0, 3.0, 4.0];
    let mut canonical = local_diagnostic_presentation(
        7,
        0,
        7,
        render_api::PoseGeneration::new(5),
        old_feet,
        30.0,
        0.0,
    )
    .unwrap();
    canonical.head_over_body = 10.0;
    canonical.submission.world_from_actor = death_tilted(
        scaled_axes(canonical.submission.world_from_actor, axes),
        Some(0.4),
    );
    let diagnostic = local_diagnostic_presentation(
        7,
        0,
        7,
        render_api::PoseGeneration::new(5),
        new_feet,
        90.0,
        0.0,
    )
    .unwrap();
    let local =
        local_actor_presentation_for_visibility(7, 7, Some(canonical), Some(diagnostic), 90.0)
            .unwrap();
    let expected = death_tilted(
        scaled_axes(rig_world_from_actor(new_feet, 80.0, 1.0), axes),
        Some(0.4),
    );
    for (actual, expected) in local
        .submission
        .world_from_actor
        .into_iter()
        .flatten()
        .zip(expected.into_iter().flatten())
    {
        assert!((actual - expected).abs() < 1e-5, "{actual} != {expected}");
    }
}
