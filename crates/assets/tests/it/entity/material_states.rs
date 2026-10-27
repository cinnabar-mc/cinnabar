use assets::{EntityRenderMaterialState, RuntimeEntityAssets, encode_entity_blob};

use super::suite::carrier_v4_fixture;

#[test]
fn material_shading_round_trips_through_entity_carrier() {
    let mut compiled = carrier_v4_fixture();
    for bits in 0..1024 {
        let state = EntityRenderMaterialState {
            alpha_test: bits & 1 != 0,
            emissive: bits & 2 != 0,
            disable_overlay: bits & 4 != 0,
            only_emissive: bits & 8 != 0,
            color_mask: bits & 16 != 0,
            multitexture: bits & 32 != 0,
            glint: bits & 64 != 0,
            texture_tint: bits & 128 != 0,
            texture_mask: bits & 256 != 0,
            color_second_texture: bits & 512 != 0,
        };
        compiled.render.layers[0].material_state = Some(state);
        let encoded = encode_entity_blob(&compiled).unwrap();
        let runtime = RuntimeEntityAssets::decode(&encoded).unwrap();
        assert_eq!(runtime.render_data().layers[0].material_state, Some(state));
        assert_eq!(runtime.encode().unwrap().as_ref(), encoded.as_ref());
    }
}

/// Reads an explicitly named carrier fixture, skipping only when it is unavailable.
#[expect(
    clippy::print_stderr,
    reason = "names the missing fixture when the test skips"
)]
fn carrier_fixture(variable: &str) -> Option<Vec<u8>> {
    let Some(path) = std::env::var_os(variable) else {
        eprintln!("missing fixture {variable}: skipping entity carrier check");
        return None;
    };
    match std::fs::read(&path) {
        Ok(encoded) => Some(encoded),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("missing fixture {variable}={path:?}: skipping entity carrier check");
            None
        }
        Err(error) => panic!("read entity carrier {variable}={path:?}: {error}"),
    }
}

#[test]
fn existing_entity_carrier_keeps_encoded_identity() {
    let Some(encoded) = carrier_fixture("CINNABAR_ENTITY_CARRIER") else {
        return;
    };
    let runtime = RuntimeEntityAssets::decode(&encoded).expect("decode existing entity carrier");
    assert!(!runtime.render_data().layers.is_empty());
    assert_eq!(runtime.encode().unwrap().as_ref(), encoded.as_slice());
}

#[test]
fn material_choices_preserve_alpha_for_every_possible_raster() {
    use assets::{
        EntityRenderMaterial, EntityRenderMaterialSelection as Node,
        entity::material::{EntityMaterialDefinition, EntityMaterialState},
    };
    let mut compiled = carrier_v4_fixture();
    let source = compiled.render.candidates[0].source;
    compiled.render.layers[0].material_state = None;
    compiled.render.layers[0].material_selection = Some(0);
    compiled.render.materials = vec![EntityMaterialDefinition {
        states: Box::new([EntityMaterialState::Blending]),
        ..Default::default()
    }]
    .into();
    compiled.render.material_selections = vec![
        Node::Branch {
            condition: 0,
            yes: 1,
            no: 2,
        },
        Node::Raster {
            material: EntityRenderMaterial::Default,
            state: None,
            definition: None,
            masks: None,
        },
        Node::Raster {
            material: EntityRenderMaterial::Default,
            state: Some(EntityRenderMaterialState::default()),
            definition: Some(0),
            masks: None,
        },
    ]
    .into();
    assert!(assets::actor_sources_keeping_alpha(&compiled.render).contains(&source));
    let encoded = encode_entity_blob(&compiled).unwrap();
    let runtime = RuntimeEntityAssets::decode(&encoded).unwrap();
    assert_eq!(
        runtime
            .render_data()
            .materials(&runtime.render_data().layers[0])
            .count(),
        2
    );
    assert_eq!(runtime.encode().unwrap().as_ref(), encoded.as_ref());
}

#[test]
fn material_choices_share_the_multitexture_page_when_any_choice_multitextures() {
    use assets::{EntityRenderMaterial, EntityRenderMaterialSelection as Node};
    let mut compiled = carrier_v4_fixture();
    let source = compiled.render.candidates[0].source;
    compiled.render.layers[0].material_state = None;
    compiled.render.layers[0].material_selection = Some(0);
    compiled.render.material_selections = vec![
        Node::Branch {
            condition: 0,
            yes: 1,
            no: 2,
        },
        Node::Raster {
            material: EntityRenderMaterial::Default,
            state: None,
            definition: None,
            masks: None,
        },
        Node::Raster {
            material: EntityRenderMaterial::Default,
            state: Some(EntityRenderMaterialState {
                multitexture: true,
                ..Default::default()
            }),
            definition: None,
            masks: None,
        },
    ]
    .into();
    assert!(assets::actor_multitexture_sources(&compiled.render).contains(&source));
    compiled.render.material_selections = vec![Node::Raster {
        material: EntityRenderMaterial::Default,
        state: None,
        definition: None,
        masks: None,
    }]
    .into();
    assert!(!assets::actor_multitexture_sources(&compiled.render).contains(&source));
}
