//! Generated item geometry remains drawable and disjoint from reserved slots.

use super::*;

#[test]
fn review_render_pack_replacement_reclaims_mesh_slots_without_reusing_vanilla_ids() {
    let (mut runtime, _, _) = block_fixture();
    runtime.next_mesh = STARTUP_MESH_ID_CAPACITY as u32;
    for index in [0, STARTUP_MESH_ID_CAPACITY as u32 - 2] {
        runtime
            .meshes
            .insert(MeshKey::Block(index), Some(item_mesh_rig_id(index)));
    }
    let retired = item_mesh_rig_id(STARTUP_MESH_ID_CAPACITY as u32 - 1);
    let placement = runtime.blocks.source.placements[0].unwrap();
    for _ in 0..8 {
        runtime.attachable_meshes.insert((true, 0, (1, 0)), retired);
        runtime.set_pack_layer(None);
        assert_eq!(
            runtime.build_mesh(MeshKey::Block(7), 0, placement),
            Some(retired)
        );
        assert_eq!(
            runtime.meshes[&MeshKey::Block(0)],
            Some(item_mesh_rig_id(0))
        );
    }
}

#[test]
fn many_distinct_held_items_remain_drawable_and_reuse_their_meshes() {
    let (mut runtime, body, mut item) = block_fixture();
    let count = 1024;
    Arc::make_mut(&mut runtime.blocks.source)
        .sheets
        .extend((0..count).map(|visual| (visual, 0)));
    let mut rigs = Vec::new();
    for visual in 0..count {
        item.kind = HeldKind::Block(visual);
        let draw = runtime
            .first_person_item(&body, &item, ItemAnimationState::default())
            .unwrap_or_else(|| panic!("held item {visual} disappeared"));
        rigs.push(draw.presentation.submission.input.rig);
        let geometry = runtime.take_pending_geometries();
        assert_eq!(geometry.len(), 1);
        assert_eq!(geometry[0].id, *rigs.last().unwrap());
    }
    assert_eq!(
        rigs.iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        count as usize
    );
    for (visual, expected) in (0..count).zip(rigs) {
        item.kind = HeldKind::Block(visual);
        let draw = runtime
            .first_person_item(&body, &item, ItemAnimationState::default())
            .unwrap();
        assert_eq!(draw.presentation.submission.input.rig, expected);
    }
    assert!(runtime.take_pending_geometries().is_empty());
}
