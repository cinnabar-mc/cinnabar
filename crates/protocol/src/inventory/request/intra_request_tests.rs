use super::*;

#[test]
fn partial_move_request_matches_pinned_gophertunnel_bytes() {
    let fixture = &include_bytes!("../../../fixtures/item_stack_request_partial_moves.bin")[..];
    let expected = crate::decode_batch(bytes::Bytes::copy_from_slice(fixture))
        .unwrap()
        .remove(0);
    let actions: Vec<_> = [(2, 5, 100, 10), (3, 11, -3, 11), (5, 0, -3, 0)]
        .into_iter()
        .map(
            |(amount, destination, source_id, destination_id)| StackRequestAction::Place {
                amount,
                source: StackRequestSlot {
                    container: StackRequestContainer::LevelEntity { dynamic_id: None },
                    slot: 2,
                    stack_network_id: source_id,
                },
                destination: player(destination, destination_id),
            },
        )
        .collect();
    let mut packet = item_stack_request_packet(-3, &actions).unwrap();
    packet.header = expected.header;
    assert_eq!(crate::encode(&packet).unwrap().as_ref(), fixture);
}

/// A player slot with an explicit stack or prediction identity.
fn player(slot: u8, id: i32) -> StackRequestSlot {
    StackRequestSlot {
        container: StackRequestContainer::PlayerInventory,
        slot,
        stack_network_id: id,
    }
}

#[test]
fn current_request_references_require_a_preceding_slot_change() {
    let first = StackRequestAction::Place {
        amount: 2,
        source: player(0, 100),
        destination: player(1, 200),
    };
    let drop = |source| StackRequestAction::Drop {
        amount: 1,
        source,
        randomly: false,
    };
    assert!(item_stack_request_packet(-3, &[first.clone(), drop(player(0, -3))]).is_ok());
    assert!(item_stack_request_packet(-3, &[first.clone(), drop(player(1, -3))]).is_ok());
    for source in [player(2, -3), player(0, -5)] {
        assert!(item_stack_request_packet(-3, &[first.clone(), drop(source)]).is_err());
    }
    assert!(item_stack_request_packet(-3, &[drop(player(0, -3)), first.clone()]).is_err());
    let mut aliased = player(1, -3);
    aliased.container = StackRequestContainer::CombinedInventory;
    assert!(item_stack_request_packet(-3, &[first, drop(aliased)]).is_ok());
}

#[test]
fn request_dependencies_do_not_cross_container_instances() {
    let source = |dynamic_id, stack_network_id| StackRequestSlot {
        container: StackRequestContainer::LevelEntity {
            dynamic_id: Some(dynamic_id),
        },
        slot: 2,
        stack_network_id,
    };
    let first = StackRequestAction::Take {
        amount: 1,
        source: source(7, 100),
        destination: player(0, 0),
    };
    let second = StackRequestAction::Take {
        amount: 1,
        source: source(8, -3),
        destination: player(1, 0),
    };
    assert!(item_stack_request_packet(-3, &[first, second]).is_err());
}
