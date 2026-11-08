use super::super::*;
use protocol_ids::{ActorDataKey, ActorFlag};

#[test]
fn masked_multitexture_ignores_unused_samplers_and_draws_a_lone_texture() {
    use assets::{
        EntityAssetSource, EntityRenderCandidate, EntityRenderMaterialState, EntityRenderSlot,
    };
    let identifier = "custom:profession";
    for count in [1, 2, 3] {
        let mut compiled = crate::test_support::counting_random_compiled(identifier);
        let base = compiled.render.candidates[0].source;
        let mut sources = compiled.sources.into_vec();
        let clothes = sources.len() as u32;
        for name in ["z_clothes", "z_unused"] {
            sources.push(EntityAssetSource {
                path: format!("textures/items/{name}.png").into(),
                source_bytes: 1,
                source_sha256: [2; 32],
            });
        }
        compiled.sources = sources.into();
        compiled.render.layers[0].material_state = Some(EntityRenderMaterialState {
            masked_multitexture: true,
            ..Default::default()
        });
        compiled.render.layers[0].slot_count = count;
        compiled.render.slots = (0..u32::from(count))
            .map(|index| EntityRenderSlot {
                first_candidate: index,
                candidate_count: 1,
                default_candidate: None,
            })
            .collect();
        compiled.render.candidates = [base, clothes, clothes + 1]
            .into_iter()
            .take(usize::from(count))
            .map(|source| EntityRenderCandidate {
                condition: None,
                source,
            })
            .collect();
        let assets =
            RuntimeEntityAssets::from_compiled(compiled.with_unresolved_admissions()).unwrap();
        let mut actor = tests::actor_with_metadata(HashMap::new());
        actor.kind = ActorKind::Entity {
            identifier: identifier.into(),
        };
        let mut store = ActorAnimationStore::with_assets(Arc::new(assets));
        store.insert(1, 0, &actor);
        store.advance_tick(
            &HashMap::from([(actor.runtime_id, actor.clone())]),
            None,
            None,
            true,
            true,
            |_| ActorTickContext::default(),
        );
        let snapshot = store.get(actor.runtime_id).unwrap();
        assert_eq!(snapshot.render.len(), 1);
        assert_eq!(snapshot.render[0].source, base);
        let expected = (count > 1).then_some([clothes; 2]);
        assert_eq!(
            snapshot.render[0].multitexture, expected,
            "{count} samplers"
        );
    }
}

#[test]
fn pinned_villager_masked_multitexture_tracks_profession_changes() {
    let Some(assets) = crate::test_support::entity_assets(
        "CINNABAR_ENTITY_CARRIER",
        "pinned_villager_masked_multitexture_tracks_profession_changes",
    ) else {
        return;
    };
    for (identifier, folder, unemployed_masked) in [
        ("minecraft:villager_v2", "villager2", false),
        ("minecraft:zombie_villager_v2", "zombie_villager2", true),
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
                if baby || (profession == 0 && !unemployed_masked) {
                    assert!(
                        clothing.multitexture.is_none(),
                        "{identifier}: unmasked clothing"
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
