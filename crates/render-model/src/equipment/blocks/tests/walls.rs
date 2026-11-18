use super::*;

#[test]
fn carried_walls_preserve_directional_materials_on_posts_and_rails() {
    let mut source = overlay(BlockFlags::empty(), 0, 16);
    source.materials = vec![source.materials[1]; BlockFace::ALL.len()];
    source.visuals[0].faces = BlockFace::ALL.map(|face| face as u32);
    source.visuals[0].kind = VisualKind::Model;
    source.visuals[0].model_template = 0;
    source.light_properties = vec![assets::LightProperties::OPAQUE_DARK];
    source.model_templates.push(assets::ModelTemplate {
        quad_start: 0,
        quad_count: 1,
        flags: 0,
    });
    source.model_quads.push(assets::ModelQuad {
        positions: [[0, 0, 0], [0, 256, 0], [256, 256, 0], [256, 0, 0]],
        uvs: [[0, 4096], [0, 0], [4096, 0], [4096, 4096]],
        material: 0,
        flags: 0,
    });
    let base = RuntimeAssets::diagnostic();
    let visual = base.visual_count() as u32;
    let world = base.with_block_overlay(visual, &source).unwrap();
    let block = world.resolve(NetworkIdMode::Sequential, visual);
    let expected = BlockFace::ALL.map(|face| block.face(face).material_id());
    let (materials, vertices) = super::super::walls::model(&world, visual).unwrap();
    assert_eq!(vertices.len(), 144);
    let rects = face_rects([0., 0., 1., 1.]);
    for vertex in vertices {
        let face = match vertex.normal {
            [-1., 0., 0.] => BlockFace::West,
            [1., 0., 0.] => BlockFace::East,
            [0., -1., 0.] => BlockFace::Down,
            [0., 1., 0.] => BlockFace::Up,
            [0., 0., -1.] => BlockFace::North,
            [0., 0., 1.] => BlockFace::South,
            normal => panic!("unexpected wall normal {normal:?}"),
        };
        let slot = rects
            .iter()
            .position(|rect| vertex.glint_origin == [rect[0], rect[1]])
            .unwrap();
        assert_eq!(materials[slot], expected[face as usize], "{face:?}");
    }
}

#[test]
fn carried_walls_keep_two_posts_and_two_rail_halves_on_their_carried_axis() {
    let Some((world, entities, icons)) = pinned_carriers() else {
        return;
    };
    let held = collect_held(&world, &entities, &icons);
    let mut checked = 0;
    for definition in entities.item_visuals() {
        let ItemVisualDefinitionRoute::BlockItem { block_visual } = definition.route else {
            continue;
        };
        if assets::registry::pinned_block_model_family(block_visual.0)
            != Some(assets::ModelFamily::Wall)
            || !assets::registry::pinned_block_identity(block_visual.0)
                .is_some_and(|(name, _)| name == definition.key.identifier.as_ref())
        {
            continue;
        }
        let name = definition.key.identifier.as_ref();
        let vertices = held
            .models
            .get(&block_visual.0)
            .unwrap_or_else(|| panic!("{name} holds its wall model"));
        assert_eq!(vertices.len(), 144, "{name}");
        for (axis, half_extent) in [0.25, 0.5, 0.75].into_iter().enumerate() {
            let min = vertices
                .iter()
                .map(|v| v.position[axis])
                .fold(f32::INFINITY, f32::min);
            let max = vertices
                .iter()
                .map(|v| v.position[axis])
                .fold(f32::NEG_INFINITY, f32::max);
            assert_eq!(
                [min, max],
                [-half_extent, half_extent],
                "{name} axis {axis}"
            );
        }
        checked += 1;
    }
    assert!(checked > 0);
}
