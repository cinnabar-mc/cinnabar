use super::MaterialKeys;

// The sidecar round-trips and refuses a carrier of a different size.
#[test]
fn json_round_trips_and_checks_the_material_count() {
    let keys = MaterialKeys::from_entries([(3, "stone"), (1, "stone"), (2, "dirt"), (1, "stone")]);
    assert_eq!(keys.materials("stone"), [1, 3]);
    assert_eq!(keys.materials("missing"), [] as [u32; 0]);
    let json = keys.to_json(4);
    assert_eq!(MaterialKeys::from_json(&json, 4), Some(keys));
    assert_eq!(MaterialKeys::from_json(&json, 5), None);
    assert_eq!(MaterialKeys::from_json(&json, 3), None);
}

// Packaged installs resolve vanilla aliases from the sidecar; a pre-alias one is stale.
#[test]
fn aliases_round_trip_and_alias_free_sidecars_are_rejected() {
    let keys = MaterialKeys::from_entries([(0, "stone")])
        .with_aliases([("stone", "textures/blocks/stone"), ("unused", "textures/x")])
        .with_fixed_tints([("stone", [20, 80, 30])]);
    let decoded = MaterialKeys::from_json(&keys.to_json(1), 1).unwrap();
    assert_eq!(
        decoded.fixed_tints().collect::<Vec<_>>(),
        [("stone", [20, 80, 30])]
    );
    assert_eq!(
        decoded.aliases().collect::<Vec<_>>(),
        [("stone", "textures/blocks/stone"), ("unused", "textures/x")]
    );
    assert_eq!(
        MaterialKeys::from_json(br#"{"schema":1,"materials":1,"keys":{"stone":[0]}}"#, 1),
        None
    );
}

#[test]
fn complete_sources_round_trip_with_variant_and_carried_binding_identity() {
    use super::{BlockTextureSource, MaterialTextureSource};
    let keys = MaterialKeys::from_entries([(1, "fixture")]).with_aliases([("fixture", "textures/a")])
        .with_sources(
            [("fixture".into(), serde_json::json!({"textures":["textures/a",{"variations":["textures/b","textures/c"]}]}))],
            [(1, MaterialTextureSource { key:"fixture".into(), variant:1, path:"textures/b".into(), raster: "textures/b.png".into(), baked_tint:Some([120,80,40]) })],
            [BlockTextureSource { visual:2, name:"minecraft:fixture".into(), binding_name:"fixture".into(),
                source_faces:[2,3,1,0,4,5], source_rotations:[false,false,true,true,true,true], cube_binding_eligible:true, carried_keys:std::array::from_fn(|_| Some("carried".into())),
                carried_fallback_keys:std::array::from_fn(|_| Some("fixture".into())) }],
        )
        .with_raster_sizes([("textures/b.png".into(), [16, 32])]);
    let restored = MaterialKeys::from_json(&keys.to_json(3), 3).unwrap();
    assert_eq!(restored, keys);
    assert_eq!(restored.material_source(1).unwrap().variant, 1);
    assert_eq!(restored.raster_size("textures/b.png"), Some([16, 32]));
    assert_eq!(
        restored.material_source(1).unwrap().raster.as_ref(),
        "textures/b.png"
    );
    assert_eq!(
        restored.material_source(1).unwrap().baked_tint,
        Some([120, 80, 40])
    );
    assert_eq!(
        restored.block_sources()[0].source_rotations,
        [false, false, true, true, true, true]
    );
    assert_eq!(restored.block_sources()[0].source_faces, [2, 3, 1, 0, 4, 5]);
    let mut bad: serde_json::Value = serde_json::from_slice(&keys.to_json(3)).unwrap();
    bad["block_sources"][0]["source_faces"][0] = 6.into();
    assert!(MaterialKeys::from_json(&serde_json::to_vec(&bad).unwrap(), 3).is_none());
    bad["block_sources"][0]["source_faces"][0] = 3.into();
    assert!(MaterialKeys::from_json(&serde_json::to_vec(&bad).unwrap(), 3).is_none());
}

#[test]
fn older_unordered_source_arrays_are_normalized_when_decoded() {
    let keys = MaterialKeys::default().with_sources(
        [],
        [],
        [0, 1].map(|visual| super::BlockTextureSource {
            visual,
            name: "minecraft:fixture".into(),
            binding_name: "fixture".into(),
            source_faces: [0, 1, 2, 3, 4, 5],
            source_rotations: [false; 6],
            cube_binding_eligible: true,
            carried_keys: std::array::from_fn(|_| None),
            carried_fallback_keys: std::array::from_fn(|_| None),
        }),
    );
    let mut wire: serde_json::Value = serde_json::from_slice(&keys.to_json(1)).unwrap();
    wire["block_sources"].as_array_mut().unwrap().reverse();
    let decoded = MaterialKeys::from_json(&serde_json::to_vec(&wire).unwrap(), 1).unwrap();
    assert_eq!(decoded.block_sources(), keys.block_sources());
}

#[test]
fn animation_sources_and_destinations_survive_sidecar_round_trip() {
    let source: crate::texture::flipbook::FlipbookSource =
        serde_json::from_value(serde_json::json!({
            "flipbook_texture":"textures/strip", "atlas_tile":"tile", "frames":[2,0,1],
            "atlas_index":3, "atlas_tile_variant":2, "ticks_per_frame":7,
            "replicate":2, "blend_frames":false
        }))
        .unwrap();
    let keys = MaterialKeys::from_entries([(1, "tile"), (2, "tile")])
        .with_aliases([("tile", "textures/static")])
        .with_flipbooks(std::slice::from_ref(&source))
        .with_flipbook_selectors([(1, (3, 2)), (2, (3, 1))]);
    let bytes = keys.to_json(3);
    assert_eq!(MaterialKeys::from_json(&bytes, 3), Some(keys));
    for (field, value) in [
        ("ticks_per_frame", serde_json::json!(0)),
        ("replicate", serde_json::json!(0)),
        ("frames", serde_json::json!([0, 1])),
    ] {
        let mut wire: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        wire["flipbooks"][0][field] = value;
        if field == "frames" {
            wire["flipbooks"][0]["ticks_per_frame"] = serde_json::json!(u32::MAX);
        }
        assert!(MaterialKeys::from_json(&serde_json::to_vec(&wire).unwrap(), 3).is_none());
    }
}
