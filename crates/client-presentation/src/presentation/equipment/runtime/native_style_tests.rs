//! Animation-style routing for texture-only pack bows and crossbows.

use super::*;

#[test]
fn texture_only_pack_bows_and_crossbows_keep_their_attachable_pose_in_bedrock_mode() {
    for (identifier, geometry) in [
        ("minecraft:bow", "geometry.bow_standby"),
        ("minecraft:crossbow", "geometry.crossbow_standby"),
    ] {
        let (mut runtime, pages) = startup_runtime(fixture(identifier, geometry, 10, true));
        let pages = install(&mut runtime, pages, fixture(identifier, geometry, 80, true));
        assert!(
            runtime.uses_native_item_mesh(identifier, true),
            "{identifier} stays native under Java's hand stack"
        );
        assert!(
            !runtime.uses_native_item_mesh(identifier, false),
            "{identifier} keeps its attachable in Bedrock mode"
        );
        let body = player_body(&mut runtime);
        let owner = owner();
        let names = [Box::from("rightItem")];
        let rig = owner_rig(&owner, &names);
        let equipment = ActorEquipmentInput {
            main: Some(item(identifier)),
            ..Default::default()
        };
        let expected = runtime
            .first_person_attachable(
                &body,
                equipment.main.as_ref().unwrap(),
                &owner,
                &rig,
                equipment.attachable_input(AttachableAnimationInput {
                    frame_alpha: 1.0,
                    ..Default::default()
                }),
                None,
            )
            .expect("Bedrock mode draws the pack weapon through its attachable");
        assert_eq!(
            selected_pixel(&pages, &expected.presentation)[0],
            80,
            "{identifier} keeps the pack's artwork"
        );
        let layers = runtime.layers_for(
            &body,
            &equipment,
            Some(EquipmentAnimation {
                owner: &owner,
                rig: &rig,
                frame_alpha: 1.0,
                delta_seconds: 0.05,
            }),
        );
        assert_eq!(layers.len(), 1);
        assert_eq!(
            layers[0].submission.input.rig, expected.presentation.submission.input.rig,
            "{identifier} draws its attachable mesh in third person"
        );
        assert_eq!(
            layers[0].submission.input.current_bones,
            expected.presentation.submission.input.current_bones,
            "{identifier} keeps the attachable's wield pose in third person"
        );
    }
}

#[test]
fn bedrock_pack_crossbow_shows_charging_and_loaded_frames_in_third_person() {
    use assets::item_use::ItemProjectileRole;

    let (identifier, geometry) = ("minecraft:crossbow", "geometry.crossbow_standby");
    let (mut runtime, pages) = startup_runtime(fixture(identifier, geometry, 10, true));
    let mut files = fixture(identifier, geometry, 80, true);
    let (_, animation) = files
        .iter_mut()
        .find(|(path, _)| path.as_ref() == "animations/weapon.json")
        .unwrap();
    *animation = serde_json::to_vec(&serde_json::json!({
        "format_version": "1.10.0", "animations": {"animation.fixture.wield": {
            "loop": true, "bones": {"rightitem": {
                "position": ["query.get_animation_frame", "query.item_is_charged", 0]
            }}
        }}
    }))
    .unwrap();
    install(&mut runtime, pages, files);
    assert!(!runtime.uses_native_item_mesh(identifier, false));
    let body = player_body(&mut runtime);
    let names = [Box::from("rightItem")];
    let mut owner = owner();
    let mut draw = |elapsed: Option<u32>, projectile: Option<ItemProjectileRole>| {
        owner.metadata.insert(
            0,
            protocol::ActorMetadataValue::Flags(if elapsed.is_some() { 1 << 4 } else { 0 }),
        );
        let mut rig = owner_rig(&owner, &names);
        rig.hand[1].use_ticks = elapsed.unwrap_or_default();
        let mut stack = item(identifier);
        stack.use_facts.ticks = Some(25);
        stack.charged_projectile = projectile;
        let equipment = ActorEquipmentInput {
            main: Some(stack),
            ..Default::default()
        };
        let layers = runtime.layers_for(
            &body,
            &equipment,
            Some(EquipmentAnimation {
                owner: &owner,
                rig: &rig,
                frame_alpha: 0.5,
                delta_seconds: 0.016,
            }),
        );
        assert_eq!(layers.len(), 1);
        layers[0].submission.input.current_bones[0].translation_scale
    };
    let idle = draw(None, None);
    for (elapsed, projectile, frame, charged) in [
        (Some(8), None, 1.0, 0.0),
        (Some(18), None, 3.0, 0.0),
        (None, Some(ItemProjectileRole::Arrow), 4.0, 1.0),
        (None, Some(ItemProjectileRole::Firework), 5.0, 1.0),
    ] {
        let pose = draw(elapsed, projectile);
        assert!(
            ((pose[0] - idle[0]).abs() - frame / 16.0).abs() < 1e-5,
            "{elapsed:?}/{projectile:?} must select crossbow frame {frame}"
        );
        assert!(
            ((pose[1] - idle[1]).abs() - charged / 16.0).abs() < 1e-5,
            "{elapsed:?}/{projectile:?} must report charged {charged}"
        );
    }
}
