use super::*;

#[test]
fn pumpkin_inventory_icons_face_south_whichever_state_the_item_routes_to() {
    let root = tempfile::tempdir().unwrap();
    let blocks = IconBlocks::read(root.path()).unwrap();
    let mut seen = std::collections::BTreeSet::new();
    for record in blocks.records.iter().filter(|record| {
        matches!(
            assets::facts::pack::face_slots(&record.name),
            Some(assets::facts::pack::FaceSlots::Pumpkin)
        )
    }) {
        let pose =
            &blocks.records[blocks.icon_state(BlockVisualId(record.sequential_id)).0 as usize];
        let state: serde_json::Value = serde_json::from_str(&pose.canonical_state).unwrap();
        assert_eq!(pose.name, record.name);
        assert_eq!(
            state["minecraft:cardinal_direction"]["value"], "south",
            "{} {}",
            record.name, record.canonical_state
        );
        seen.insert(record.name.as_ref());
    }
    assert_eq!(
        seen.into_iter().collect::<Vec<_>>(),
        [
            "minecraft:carved_pumpkin",
            "minecraft:lit_pumpkin",
            "minecraft:pumpkin"
        ]
    );
}
