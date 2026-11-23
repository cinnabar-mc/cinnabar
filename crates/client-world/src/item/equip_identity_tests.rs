use super::*;

fn vanilla_network_id(identifier: &str) -> i32 {
    protocol::vanilla_item_registry()
        .iter()
        .find(|item| item.identifier.as_ref() == identifier)
        .unwrap()
        .network_id
}

/// The held identity animation compares ignores count and reads durability from stack tags.
#[test]
fn canonical_stacks_drop_count_and_read_tag_damage() {
    let store = ItemStateStore::diagnostic();
    let mut stack = NetworkItemStack {
        network_id: vanilla_network_id("minecraft:potion"),
        count: 2,
        metadata: 21,
        ..Default::default()
    };
    let before = store.canonicalize(&stack).unwrap();
    stack.count = 1;
    let after = store.canonicalize(&stack).unwrap();
    assert_eq!(before.identifier, after.identifier);
    assert_eq!(before.identity.metadata, after.identity.metadata);

    let network_id = vanilla_network_id("minecraft:iron_sword");
    for damage in [0_u32, 10] {
        let mut extra = vec![0xff, 0xff, 1, 10, 0, 0, 3, 6, 0];
        extra.extend_from_slice(b"Damage");
        extra.extend_from_slice(&damage.to_le_bytes());
        extra.push(0);
        let item = store
            .canonicalize(&NetworkItemStack {
                network_id,
                count: 1,
                nbt_digest: Sha256::digest(&extra).into(),
                extra_data: extra.into(),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(item.damage, Some(damage));
    }
}
