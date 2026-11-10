use super::*;

const TOP_GEOMETRY: &str = r#"{"format_version": "1.12.0", "minecraft:geometry": [{
    "description": {"identifier": "geometry.top", "texture_width": 16, "texture_height": 16},
    "bones": [{"name": "body", "cubes": [{"origin": [-8, 0, -8], "size": [16, 16, 16],
        "uv": {"up": {"uv": [0, 0], "uv_size": [8, 4]}}}]}]},
    {"description": {"identifier": "geometry.box", "texture_width": 16, "texture_height": 16},
    "bones": [{"name": "body", "cubes": [{"origin": [-8, 0, -8], "size": [4, 4, 4],
        "uv": [0, 0]}]}]}]}"#;

/// A top face's model corners paired with UVs, for one geometry and legacy flag.
fn top_corners(identifier: &str, legacy: bool) -> [([i16; 3], [u16; 2]); 4] {
    let view = view_with_catalog(
        TOP_GEOMETRY.as_bytes(),
        r#"{"texture_data": {"lucky": {"textures": "textures/blocks/lucky"}}}"#,
    );
    let blocks = CustomBlocks {
        blocks: Arc::from([block(
            "test:top",
            1,
            CustomBlockVisuals {
                base: CustomVisualComponents {
                    geometry: Some(identifier.into()),
                    geometry_needs_legacy_top_rotation: legacy,
                    materials: materials("lucky"),
                    ..Default::default()
                },
                ..Default::default()
            },
        )]),
        ..Default::default()
    };
    let compiled = compile_block_overlay(&view, &blocks, false, None).unwrap();
    let template =
        compiled.overlay.model_templates[compiled.overlay.visuals[0].model_template as usize];
    let up = compiled.overlay.model_quads[template.quad_start as usize..]
        [..template.quad_count as usize]
        .iter()
        .find(|quad| {
            quad.flags & assets::MODEL_QUAD_FLAG_FACE_MASK
                == assets::BlockFace::Up.model_quad_face_id()
        })
        .unwrap();
    std::array::from_fn(|corner| (up.positions[corner], up.uvs[corner]))
}

fn uv_at(corners: &[([i16; 3], [u16; 2]); 4], x: i16, z: i16) -> [u16; 2] {
    corners
        .iter()
        .find(|(position, _)| position[0] == x && position[2] == z)
        .unwrap()
        .1
}

#[test]
fn legacy_top_rotation_keeps_the_older_per_face_top() {
    let current = top_corners("geometry.top", false);
    assert_eq!(
        uv_at(&current, 256, 256),
        [0, 0],
        "south-east starts the top"
    );
    assert_eq!(uv_at(&current, 0, 0), [2048, 1024]);
    let legacy = top_corners("geometry.top", true);
    assert_eq!(uv_at(&legacy, 0, 0), [0, 0], "north-west starts the top");
    assert_eq!(uv_at(&legacy, 256, 256), [2048, 1024]);
}

#[test]
fn legacy_top_rotation_leaves_box_mapped_tops_unchanged() {
    assert_eq!(
        top_corners("geometry.box", false),
        top_corners("geometry.box", true)
    );
}

#[test]
fn legacy_top_rotation_belongs_to_the_geometry_component() {
    for (geometry, expected) in [(Some("geometry.other".into()), false), (None, true)] {
        let block = block(
            "test:top",
            1,
            CustomBlockVisuals {
                base: CustomVisualComponents {
                    geometry: Some("geometry.top".into()),
                    geometry_needs_legacy_top_rotation: true,
                    ..Default::default()
                },
                permutations: Box::new([CustomPermutation {
                    physical: Default::default(),
                    condition: "1".into(),
                    components: CustomVisualComponents {
                        geometry,
                        ..Default::default()
                    },
                }]),
                ..Default::default()
            },
        );
        let state = crate::runtime::network::block_overlay::condition::state_visual(
            &block,
            &crate::runtime::network::block_overlay::condition::BlockExpressions::new(&block),
            Some(&[]),
            &mut crate::runtime::network::block_overlay::OverlayGaps::default(),
        );
        assert_eq!(
            state.components.geometry_needs_legacy_top_rotation,
            expected
        );
    }
}
