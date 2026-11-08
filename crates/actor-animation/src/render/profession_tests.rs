use super::super::*;
use protocol_ids::{ActorDataKey, ActorFlag};

#[test]
fn pinned_villager_masked_multitexture_tracks_profession_changes() {
    let Some(assets) = crate::test_support::entity_assets(
        "CINNABAR_ENTITY_CARRIER",
        "pinned_villager_masked_multitexture_tracks_profession_changes",
    ) else {
        return;
    };
    for (identifier, folder) in [
        ("minecraft:villager_v2", "villager2"),
        ("minecraft:zombie_villager_v2", "zombie_villager2"),
    ] {
        let mut actor = tests::actor_with_metadata(HashMap::new());
        actor.kind = ActorKind::Entity {
            identifier: identifier.into(),
        };
        let mut store = ActorAnimationStore::with_assets(Arc::clone(&assets));
        store.insert(1, 0, &actor);
        for (profession, name, baby) in [
            (1, "farmer", false),
            (5, "librarian", false),
            (8, "armorer", false),
            (0, "unskilled", false),
            (14, "nitwit", false),
            (5, "librarian", true),
            (5, "librarian", false),
        ] {
            actor.metadata.insert(
                ActorDataKey::Variant.wire_value(),
                ActorMetadataValue::Int(profession),
            );
            actor.metadata.insert(
                ActorDataKey::MarkVariant.wire_value(),
                ActorMetadataValue::Int(0),
            );
            actor.metadata.insert(
                ActorDataKey::Flags.wire_value(),
                ActorMetadataValue::Flags(if baby {
                    1 << ActorFlag::Baby.wire_value()
                } else {
                    0
                }),
            );
            store.advance_tick(
                &HashMap::from([(actor.runtime_id, actor.clone())]),
                None,
                None,
                true,
                true,
                |_| ActorTickContext::default(),
            );
            for alpha in [0.0, 0.5, 1.0] {
                let mut budget = MAX_MOLANG_OPS_PER_RENDER_FRAME;
                let sampled = store
                    .render_layers(&actor, alpha, [0.0; 2], [0.0; 3], &mut budget, true)
                    .unwrap();
                let clothing = sampled
                    .render
                    .iter()
                    .find(|layer| {
                        assets.sources()[layer.source as usize]
                            .path
                            .contains("/biomes/")
                    })
                    .expect("the biome clothing layer remains drawable");
                if baby || profession == 0 {
                    assert!(
                        clothing.multitexture.is_none(),
                        "{identifier}: baby or unemployed clothes"
                    );
                } else {
                    let [profession_source, _] = clothing
                        .multitexture
                        .expect("profession clothing shares the biome draw");
                    assert_eq!(
                        assets.sources()[profession_source as usize].path.as_ref(),
                        format!("textures/entity/{folder}/professions/{name}.tga")
                    );
                }
            }
        }
    }
}
