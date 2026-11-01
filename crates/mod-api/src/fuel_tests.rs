use super::*;

#[test]
fn frame_growth_preserves_independent_filter_and_handler_limits() {
    assert_eq!(TOTAL_MOD_FRAME_FUEL, 2_000_000);
    assert_eq!(ACTION_FILTER_FUEL, 200_000);
    assert_eq!(TOTAL_SOUND_HANDLER_FUEL, 400_000);
    assert_eq!(TOTAL_TELEPORT_HANDLER_FUEL, 400_000);
    assert_eq!(HANDLER_CALL_FUEL, 50_000);
}
