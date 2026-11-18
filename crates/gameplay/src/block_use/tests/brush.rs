use super::*;

/// A brush's own use succeeds on any block nothing else took, repeats while held, and
/// predicts success without swinging.
#[test]
fn a_brush_uses_itself_on_any_block_without_swinging() {
    let brush = verified(network_item(695, 0));
    let survival = GameModeCapabilities::for_mode(PlayerGameMode::Survival);
    assert_eq!(
        LocalUse::Nothing.or_brush(true, false, &brush, &survival),
        LocalUse::UseItem
    );
    // The block's own use and a placement keep the press, and other items gain nothing.
    for taken in [LocalUse::Interact, LocalUse::Place] {
        assert_eq!(taken.or_brush(true, false, &brush, &survival), taken);
    }
    assert_eq!(
        LocalUse::Nothing.or_brush(false, false, &brush, &survival),
        LocalUse::Nothing
    );
    // A block removed since it was picked leaves air, which a brush cannot use.
    assert_eq!(
        LocalUse::Nothing.or_brush(true, true, &brush, &survival),
        LocalUse::Nothing
    );
    let unable = GameModeCapabilities {
        can_build: false,
        ..survival
    };
    assert_eq!(
        LocalUse::Nothing.or_brush(true, false, &brush, &unable),
        LocalUse::Nothing
    );

    let clicked = block_fact("minecraft:suspicious_sand");
    assert!(super::super::repeats_while_held(
        &brush,
        false,
        LocalUse::UseItem,
        clicked,
        Default::default()
    ));
    let observed =
        crate::interaction_authority::FrozenBlockObservation::fixture([2, 63, 0], 1, brush);
    let packets = use_packets(
        (&observed, 9),
        [2.5, 65.62, 0.5],
        ItemUseTrigger::SimulationTick,
        LocalUse::UseItem,
        None,
        None,
        42,
        |_| true,
        5,
    );
    assert_eq!(packets.len(), 1, "no swing precedes the transaction");
    let transaction = protocol::test_support::transactions::observe_transaction(&packets[0])
        .unwrap()
        .item_use()
        .cloned()
        .unwrap();
    assert!(transaction.predicted_success);
}
