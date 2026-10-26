use super::*;

fn entity(identifier: &str) -> ActorKind {
    ActorKind::Entity {
        identifier: identifier.into(),
    }
}

fn carrying(id: i32) -> HashMap<u32, ActorMetadataValue> {
    HashMap::from([(
        ActorDataKey::CarryBlockRuntimeID.wire_value(),
        ActorMetadataValue::Int(id),
    )])
}

#[test]
fn an_enderman_draws_its_synced_carried_block() {
    let enderman = entity(assets::facts::entity::ENDERMAN_IDENTIFIER);
    assert_eq!(carried_block_runtime_id(&enderman, &carrying(42)), Some(42));
    // Hashed palettes sync ids above i32::MAX as negative ints.
    assert_eq!(
        carried_block_runtime_id(&enderman, &carrying(-2)),
        Some(u32::MAX - 1)
    );
    assert_eq!(carried_block_runtime_id(&enderman, &HashMap::new()), None);
}

#[test]
fn only_the_enderman_renderer_draws_a_carried_block() {
    assert_eq!(
        carried_block_runtime_id(&entity("minecraft:zombie"), &carrying(42)),
        None
    );
}

#[test]
fn an_unsolved_carrier_does_not_darken_its_block() {
    assert_eq!(carried_light(Some((4, 12))), (4, 12));
    assert_eq!(carried_light(Some((0, 0))), (0, 0));
    assert_eq!(carried_light(None), (15, 15));
}
