use super::super::*;
use protocol_ids::ActorFlag;

#[test]
fn pinned_trader_llama_draws_body_and_carpet_as_one_three_sampler_layer() {
    let Some(assets) = crate::test_support::entity_assets(
        "CINNABAR_ENTITY_CARRIER",
        "pinned_trader_llama_draws_body_and_carpet_as_one_three_sampler_layer",
    ) else {
        return;
    };
    for baby in [false, true] {
        for variant in 0..4 {
            let mut actor = tests::actor_with_metadata(HashMap::from([
                (2, ActorMetadataValue::Int(variant)),
                (43, ActorMetadataValue::Int(1)),
                (
                    0,
                    ActorMetadataValue::Flags(if baby {
                        1 << ActorFlag::Baby.wire_value()
                    } else {
                        0
                    }),
                ),
            ]));
            actor.kind = ActorKind::Entity {
                identifier: "minecraft:trader_llama".into(),
            };
            let mut store = ActorAnimationStore::with_assets(Arc::clone(&assets));
            store.insert(1, 0, &actor);
            store.advance_tick(
                &HashMap::from([(actor.runtime_id, actor.clone())]),
                None,
                None,
                true,
                true,
                |_| ActorTickContext::default(),
            );
            let snapshot = store
                .get(actor.runtime_id)
                .expect("the native llama rig is admitted");
            assert_eq!(
                snapshot.render.len(),
                1,
                "textures are samplers, not three coplanar draws"
            );
            let layer = &snapshot.render[0];
            let extra = layer
                .multitexture
                .expect("body, decor and empty sampler travel together");
            let multitexture = assets::actor_multitexture_sources(assets.render_data());
            for source in [layer.source, extra[0], extra[1]] {
                assert!(multitexture.contains(&source));
            }
            let body = &assets.sources()[layer.source as usize].path;
            assert_eq!(body.contains("_baby"), baby);
            let decor = &assets.sources()[extra[0] as usize].path;
            assert!(decor.contains(if baby {
                "trader_llama_baby"
            } else {
                "trader_llama_decor"
            }));
        }
    }
}

#[test]
fn materials_sample_the_controller_slots_bedrock_binds() {
    use assets::EntityRenderMaterialState as State;
    let slots = |edit: fn(&mut State)| {
        let mut state = State::default();
        edit(&mut state);
        state.extra_texture_slots()
    };
    assert_eq!(slots(|_| {}), &[] as &[usize]);
    assert_eq!(
        slots(|state| state.glint = true),
        &[] as &[usize],
        "glint draws with the client's glint raster"
    );
    assert_eq!(slots(|state| state.multitexture = true), &[1, 2]);
    assert_eq!(slots(|state| state.texture_tint = true), &[1]);
    assert_eq!(slots(|state| state.texture_mask = true), &[1]);
    assert_eq!(
        slots(|state| {
            state.texture_tint = true;
            state.glint = true;
        }),
        &[2]
    );
}
