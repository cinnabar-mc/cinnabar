use super::*;

#[test]
fn carried_bamboo_fences_keep_their_thicker_slats() {
    let Some((world, entities, icons)) = pinned_carriers() else {
        return;
    };
    let held = collect_held(&world, &entities, &icons);
    let mut checked = 0;
    for definition in entities.item_visuals() {
        let name = definition.key.identifier.as_ref();
        if assets::facts::block_render_fact(name).kind
            != assets::facts::BlockRenderKind::BambooFence
        {
            continue;
        }
        let ItemVisualDefinitionRoute::BlockItem { block_visual } = definition.route else {
            panic!("{name} has no block route");
        };
        let vertices = held.models.get(&block_visual.0).unwrap();
        let mut heights: Vec<_> = vertices.iter().map(|vertex| vertex.position[1]).collect();
        heights.sort_by(f32::total_cmp);
        heights.dedup();
        assert_eq!(heights, [-0.5, -0.25, -0.0625, 0.25, 0.4375, 0.5]);
        checked += 1;
    }
    assert!(checked > 0);
}

#[test]
fn carried_fences_join_their_rail_halves_without_internal_caps() {
    let Some((world, entities, icons)) = pinned_carriers() else {
        return;
    };
    let held = collect_held(&world, &entities, &icons);
    let mut ordinary = 0;
    let mut bamboo = 0;
    for definition in entities.item_visuals() {
        let ItemVisualDefinitionRoute::BlockItem { block_visual } = definition.route else {
            continue;
        };
        if assets::registry::pinned_block_model_family(block_visual.0)
            != Some(assets::ModelFamily::Fence)
            || !assets::registry::pinned_block_identity(block_visual.0)
                .is_some_and(|(name, _)| name == definition.key.identifier.as_ref())
        {
            continue;
        }
        let name = definition.key.identifier.as_ref();
        let vertices = held
            .models
            .get(&block_visual.0)
            .unwrap_or_else(|| panic!("{name} holds a fence model"));
        let is_bamboo = assets::facts::block_render_fact(name).kind
            == assets::facts::BlockRenderKind::BambooFence;
        assert_eq!(vertices.len(), if is_bamboo { 288 } else { 192 }, "{name}");
        for (axis, bound) in [0.125, 0.5, 0.625].into_iter().enumerate() {
            let min = vertices
                .iter()
                .map(|vertex| vertex.position[axis])
                .fold(f32::INFINITY, f32::min);
            let max = vertices
                .iter()
                .map(|vertex| vertex.position[axis])
                .fold(f32::NEG_INFINITY, f32::max);
            assert_eq!([min, max], [-bound, bound], "{name} axis {axis}");
        }
        if is_bamboo {
            bamboo += 1;
        } else {
            assert!(vertices.iter().any(|vertex| vertex.position[2] == 0.0));
            assert!(vertices.chunks_exact(6).all(|face| {
                face[0].normal[2] == 0.0 || face.iter().any(|vertex| vertex.position[2] != 0.0)
            }));
            ordinary += 1;
        }
    }
    assert!(ordinary > 0 && bamboo > 0);
}
