//! Shield cloth survives equipment admission before its numeric registry alias arrives.
use super::*;

#[test]
fn unresolved_equipment_recovers_shield_decoration_when_its_registry_arrives() {
    let mut extra = vec![0xff, 0xff, 1, 10, 0, 0, 3, 4, 0];
    extra.extend_from_slice(b"Base");
    extra.extend_from_slice(&15i32.to_le_bytes());
    extra.push(0);
    let expected = protocol::item_shield_pattern(&extra).unwrap();
    let shield = protocol::vanilla_item_registry()
        .iter()
        .find(|entry| entry.identifier.as_ref() == assets::gui_item::SHIELD_IDENTIFIER)
        .unwrap()
        .clone();
    for identifier in [assets::gui_item::SHIELD_IDENTIFIER, "minecraft:stone"] {
        let mut store = ItemStateStore::diagnostic();
        let network_id = 30_000;
        assert!(store.identifier_for_network_id(network_id).is_none());
        let actor = ActorLifetimeId {
            session_id: 1,
            dimension: 0,
            runtime_id: 7,
            spawn_revision: 1,
        };
        store.insert_spawn(
            actor,
            1,
            NetworkItemStack {
                network_id,
                count: 1,
                nbt_digest: Sha256::digest(&extra).into(),
                extra_data: Arc::from(extra.as_slice()),
                ..NetworkItemStack::empty()
            },
        );
        assert_eq!(store.pending_count(), 1);
        assert!(store.get(actor).unwrap().item.identifier.is_none());
        assert!(store.apply_registry(ItemRegistryEvent {
            entries: Arc::from([])
        }));
        assert_eq!(store.pending_count(), 1);
        assert!(store.apply_registry(ItemRegistryEvent {
            entries: Arc::from([ItemRegistryEntry {
                network_id,
                identifier: identifier.into(),
                ..shield.clone()
            }]),
        }));
        let item = &store.get(actor).unwrap().item;
        assert_eq!(store.pending_count(), 0);
        assert_eq!(item.identifier.as_deref(), Some(identifier));
        assert_eq!(
            item.shield_pattern.as_deref(),
            (identifier == assets::gui_item::SHIELD_IDENTIFIER).then_some(&expected)
        );
    }
}
