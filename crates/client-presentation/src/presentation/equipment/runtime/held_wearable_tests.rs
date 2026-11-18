//! Held and worn equipment keep their declared model categories.
use super::*;

#[test]
fn worn_attachable_categories_do_not_become_held_models() {
    // Vanilla elytra is worn by its item facts; the custom helmet by its declared wearable slot.
    for (identifier, wearable) in [
        ("minecraft:elytra", None),
        ("test:helmet", Some("slot.armor.head")),
    ] {
        let mut files = held_pack();
        let (_, attachable) = files
            .iter_mut()
            .find(|(path, _)| path.as_ref() == "attachables/held.json")
            .unwrap();
        let mut document: serde_json::Value = serde_json::from_slice(attachable).unwrap();
        document["minecraft:attachable"]["description"]["identifier"] =
            serde_json::json!(identifier);
        *attachable = serde_json::to_vec(&document).unwrap();
        let (mut runtime, _) = pack_runtime(files);
        let body = player_body(&mut runtime);
        if let Some(slot) = wearable {
            declare_wearable(&mut runtime, identifier, slot);
        }
        let owner = owner();
        let names = ["rightItem".into()];
        let rig = owner_rig(&owner, &names);
        let layers = runtime.layers_for(
            &body,
            &held(identifier),
            Some(EquipmentAnimation {
                owner: &owner,
                rig: &rig,
                frame_alpha: 1.0,
                delta_seconds: simulation_clock::TICK_DURATION.as_secs_f32(),
            }),
        );
        assert!(
            layers.is_empty(),
            "{identifier}'s worn model must not replace its missing held icon"
        );
    }
}

#[test]
fn custom_wearable_model_draws_only_in_its_effective_armor_slot() {
    let (mut runtime, _) = pack_runtime(held_pack());
    let body = player_body(&mut runtime);
    let owner = owner();
    let names = ["rightItem".into()];
    let rig = owner_rig(&owner, &names);
    let items = super::super::session_items(
        vec![(
            "test:held",
            protocol::ItemComponents {
                wearable_slot: Some("slot.armor.head".into()),
                ..Default::default()
            },
        )],
        vec![],
    );
    runtime.set_session_items(Some(&items), None, Vec::new());
    let animation = EquipmentAnimation {
        owner: &owner,
        rig: &rig,
        frame_alpha: 0.5,
        delta_seconds: simulation_clock::TICK_DURATION.as_secs_f32(),
    };
    let mut input = held("test:held");
    assert!(
        runtime
            .layers_for(&body, &input, Some(animation))
            .is_empty(),
        "a custom wearable must not draw its worn geometry in the hand"
    );
    input.armor[0] = input.main.take();
    assert_eq!(runtime.layers_for(&body, &input, Some(animation)).len(), 1);
    input.armor[1] = input.armor[0].take();
    assert!(
        runtime
            .layers_for(&body, &input, Some(animation))
            .is_empty(),
        "the model must not draw in another armor slot"
    );
}

#[test]
fn an_attachable_without_a_worn_slot_draws_in_the_armor_slot_it_occupies() {
    // Servers dress players in cosmetics by placing an ordinary item with an attachable in an
    // armor slot. Native draws that attachable there with its own mesh and animation.
    let worn_translation = |position: &str| {
        let mut files = held_pack();
        let (_, animation) = files
            .iter_mut()
            .find(|(path, _)| path.as_ref() == "animations/held.json")
            .unwrap();
        *animation = String::from_utf8(animation.clone())
            .unwrap()
            .replace("c.is_first_person ? 50 : 2", position)
            .into_bytes();
        let (mut runtime, _) = pack_runtime(files);
        let body = player_body(&mut runtime);
        runtime.take_pending_geometries();
        let owner = owner();
        let names = ["rightItem".into()];
        let rig = owner_rig(&owner, &names);
        let animation = EquipmentAnimation {
            owner: &owner,
            rig: &rig,
            frame_alpha: 1.0,
            delta_seconds: simulation_clock::TICK_DURATION.as_secs_f32(),
        };
        let mut input = held("test:held");
        let item = input.main.take();
        let mut translations = Vec::new();
        for slot in 0..input.armor.len() {
            input.armor = Default::default();
            input.armor[slot] = item.clone();
            let layers = runtime.layers_for(&body, &input, Some(animation));
            assert_eq!(
                layers.len(),
                1,
                "the attachable must draw in armor slot {slot}"
            );
            let submission = layers[0].submission.clone();
            translations.push(submission.input.current_bones[0].translation_scale);
            let pending = runtime.take_pending_geometries();
            assert!(
                slot > 0
                    || pending
                        .iter()
                        .any(|geometry| geometry.id == submission.input.rig
                            && !geometry.vertices.is_empty()),
                "the texture-mesh model must be built for the worn layer"
            );
        }
        translations
    };
    let near = worn_translation("2");
    let far = worn_translation("12");
    for (slot, (near, far)) in near.iter().zip(&far).enumerate() {
        assert_ne!(near, far, "slot {slot} must keep the authored animation");
    }
}
