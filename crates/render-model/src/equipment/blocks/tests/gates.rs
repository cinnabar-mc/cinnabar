use super::*;

#[test]
fn carried_bamboo_gate_caps_and_end_faces_retain_their_piece_regions() {
    let Some((world, entities, icons)) = pinned_carriers() else {
        return;
    };
    let held = collect_held(&world, &entities, &icons);
    let definition = entities
        .item_visuals()
        .iter()
        .find(|d| {
            assets::facts::block_render_fact(&d.key.identifier).kind
                == assets::facts::BlockRenderKind::BambooGate
        })
        .expect("pinned bamboo gate");
    let ItemVisualDefinitionRoute::BlockItem { block_visual } = definition.route else {
        panic!("bamboo gate block route")
    };
    let vertices = &held.models[&block_visual.0];
    for (normal, point, expected) in [
        ([0., 1., 0.], [1., 8., -8.], [1.96875, 0.03125]),
        ([0., -1., 0.], [1., -3., -8.], [1.96875, 0.03125]),
        ([0., 1., 0.], [1., 8., 6.], [1.96875, 1.96875]),
        ([0., 0., -1.], [1., -2., -2.], [11.03125, 11.96875]),
        ([0., 0., 1.], [1., -2., 0.], [9.96875, 11.96875]),
        ([0., 1., 0.], [1., 1., -6.], [6.96875, 1.03125]),
        ([0., -1., 0.], [1., -2., -6.], [6.96875, 1.03125]),
        ([0., 0., -1.], [1., -2., -6.], [3.03125, 5.96875]),
        ([-1., 0., 0.], [-1., 8., -6.], [1.96875, 2.03125]),
    ] {
        let vertex = vertices
            .iter()
            .find(|v| v.position == point.map(|v| v / 16.) && v.normal == normal)
            .expect("carried bamboo face corner");
        let uv = std::array::from_fn::<_, 2, _>(|axis| {
            (vertex.uv[axis] - vertex.glint_origin[axis])
                * f32::from(BLOCK_ITEM_SHEET_GRID[axis])
                * 16.
        });
        assert!(
            uv.iter()
                .zip(expected)
                .all(|(a, b)| (a - b).abs() < 0.00002),
            "bamboo face {normal:?} at {point:?}: {uv:?}"
        );
    }
}

#[test]
fn ordinary_carried_gates_keep_post_and_rail_artwork_in_the_carried_axis() {
    let Some((world, entities, icons)) = pinned_carriers() else {
        return;
    };
    let held = collect_held(&world, &entities, &icons);
    let mut checked = 0;
    for definition in entities.item_visuals() {
        let ItemVisualDefinitionRoute::BlockItem { block_visual } = definition.route else {
            continue;
        };
        let name = definition.key.identifier.as_ref();
        if assets::registry::pinned_block_model_family(block_visual.0)
            != Some(assets::ModelFamily::Gate)
            || !assets::registry::pinned_block_identity(block_visual.0)
                .is_some_and(|(pinned, _)| pinned == name)
            || assets::facts::block_render_fact(name).kind
                == assets::facts::BlockRenderKind::BambooGate
        {
            continue;
        }
        let vertices = &held.models[&block_visual.0];
        for (normal, point, expected_uv) in [
            ([0., 0., 1.], [1., 8., 8.], [9., 0.]),
            ([0., 0., -1.], [1., 8., -8.], [7., 0.]),
            ([-1., 0., 0.], [-1., 8., 8.], [16., 0.]),
            ([1., 0., 0.], [1., 8., 8.], [14., 0.]),
            ([0., 1., 0.], [1., 8., 8.], [0., 9.]),
            ([0., 1., 0.], [1., 8., -8.], [9., 0.]),
            ([0., -1., 0.], [1., -3., 8.], [14., 7.]),
            ([0., -1., 0.], [1., -3., -8.], [9., 2.]),
            ([-1., 0., 0.], [-1., 7., -6.], [2., 1.]),
            ([1., 0., 0.], [1., 7., -6.], [6., 1.]),
            ([0., 1., 0.], [1., 7., -6.], [14., 9.]),
            ([0., -1., 0.], [1., 4., -6.], [6., 7.]),
        ] {
            let position = point.map(|v| v / 16.0);
            let vertex = vertices
                .iter()
                .find(|v| v.position == position && v.normal == normal)
                .unwrap_or_else(|| panic!("{name} face {normal:?} at {point:?}"));
            let uv = std::array::from_fn::<_, 2, _>(|axis| {
                (vertex.uv[axis] - vertex.glint_origin[axis])
                    * f32::from(BLOCK_ITEM_SHEET_GRID[axis])
                    * 16.0
            });
            assert!(
                uv.iter()
                    .zip(expected_uv)
                    .all(|(a, b)| (a - b).abs() < 0.00002),
                "{name} face {normal:?} at {point:?}: {uv:?}"
            );
        }
        checked += 1;
    }
    assert!(checked > 0);
}
