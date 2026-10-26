use super::*;

#[test]
fn carried_pressure_plates_keep_their_thin_panel_and_item_pivot() {
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
            != Some(assets::ModelFamily::PressurePlate)
            || !assets::registry::pinned_block_identity(block_visual.0)
                .is_some_and(|(name, _)| name == definition.key.identifier.as_ref())
        {
            continue;
        }
        let name = definition.key.identifier.as_ref();
        let vertices = held
            .models
            .get(&block_visual.0)
            .unwrap_or_else(|| panic!("{name} holds a thin plate"));
        assert_eq!(vertices.len(), 36, "{name}");
        for (axis, expected) in [[-0.375, 0.5], [-0.03125, 0.03125], [-0.375, 0.5]]
            .into_iter()
            .enumerate()
        {
            let min = vertices
                .iter()
                .map(|v| v.position[axis])
                .fold(f32::INFINITY, f32::min);
            let max = vertices
                .iter()
                .map(|v| v.position[axis])
                .fold(f32::NEG_INFINITY, f32::max);
            assert_eq!([min, max], expected, "{name} axis {axis}");
        }
        checked += 1;
    }
    assert!(checked > 0);
}
