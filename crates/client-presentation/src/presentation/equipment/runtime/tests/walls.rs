use super::*;

#[test]
fn carried_walls_keep_their_first_person_model_origin() {
    use bevy::math::Vec3;
    let records = assets::registry::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap();
    let mut names = std::collections::BTreeSet::new();
    let (mut runtime, body, mut item) = block_fixture();
    for record in records
        .iter()
        .filter(|record| record.model_family == assets::ModelFamily::Wall)
    {
        if !names.insert(record.name.clone()) {
            continue;
        }
        Arc::make_mut(&mut runtime.blocks.source)
            .sheets
            .insert(record.sequential_id, 0);
        item.identifier = record.name.clone().into();
        item.kind = HeldKind::Block(record.sequential_id);
        for profile in [
            render_api::CombatVisuals::Legacy112,
            render_api::CombatVisuals::Modern,
        ] {
            let first = runtime
                .first_person_item_for_profile(
                    &body,
                    &item,
                    ItemAnimationState::default().into(),
                    profile,
                )
                .unwrap();
            let pose = first.presentation.submission.input.current_bones[0];
            let anchor = Vec3::from_array(pose.translation_scale[..3].try_into().unwrap());
            assert!(
                anchor.abs_diff_eq(Vec3::new(0.8428427, -0.32, -0.72), 1.0e-6),
                "{} profile={profile:?}: {anchor:?}",
                item.identifier
            );
            assert!((pose.translation_scale[3] - 0.4).abs() < 1.0e-6);
        }
        item.identifier = Arc::from("custom:ordinary");
        let ordinary = runtime
            .first_person_item(&body, &item, ItemAnimationState::default())
            .unwrap();
        let pose = ordinary.presentation.submission.input.current_bones[0];
        let anchor = Vec3::from_array(pose.translation_scale[..3].try_into().unwrap());
        assert!(anchor.abs_diff_eq(Vec3::new(0.56, -0.52, -0.72), 1.0e-6));
    }
    assert!(!names.is_empty());
}
