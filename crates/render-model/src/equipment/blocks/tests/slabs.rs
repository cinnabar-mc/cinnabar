use super::*;

#[test]
fn static_carried_slabs_are_centered_between_their_caps() {
    let Some((world, entities, icons)) = pinned_carriers() else {
        return;
    };
    let held = collect_held(&world, &entities, &icons);
    let registry = assets::registry::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap();
    let doubles: std::collections::BTreeSet<_> = registry
        .iter()
        .filter(|record| {
            record
                .traits
                .contains(assets::registry::BlockTraits::DOUBLE_SLAB)
        })
        .map(|record| record.name.as_ref())
        .collect();
    let mut checked = 0;
    for definition in entities.item_visuals() {
        let ItemVisualDefinitionRoute::BlockItem { block_visual } = definition.route else {
            continue;
        };
        let name = definition.key.identifier.as_ref();
        if assets::registry::pinned_block_model_family(block_visual.0)
            != Some(assets::ModelFamily::Slab)
            || !assets::registry::pinned_block_identity(block_visual.0)
                .is_some_and(|(pinned, _)| pinned == name)
            || icons.lookup_index(name, definition.key.metadata).is_none()
        {
            continue;
        }
        let block = world.resolve(NetworkIdMode::Sequential, block_visual.0);
        if BlockFace::ALL.into_iter().any(|face| {
            world.materials()[block.face(face).material_id() as usize].animation != NO_ANIMATION
        }) {
            continue;
        }
        let vertices = held
            .models
            .get(&block_visual.0)
            .unwrap_or_else(|| panic!("{name} holds a slab model"));
        assert_eq!(vertices.len(), 36, "{name}");
        let half_height = if doubles.contains(name) { 0.5 } else { 0.25 };
        if doubles.contains(name) {
            let (_, prior) = cube_model(&world, block_visual.0)
                .or_else(|| template_model(&world, block_visual.0))
                .unwrap();
            assert_eq!(vertices.len(), prior.len(), "{name}");
            for (actual, expected) in vertices.iter().zip(prior) {
                assert_eq!(actual.position, expected.position, "{name}");
                assert_eq!(actual.uv, expected.uv, "{name}");
                assert_eq!(actual.normal, expected.normal, "{name}");
            }
        }
        for (axis, half_extent) in [0.5, half_height, 0.5].into_iter().enumerate() {
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
