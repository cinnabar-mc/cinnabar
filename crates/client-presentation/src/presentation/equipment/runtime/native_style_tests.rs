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
