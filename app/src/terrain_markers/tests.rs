use super::*;

#[test]
fn a_creative_player_holding_a_marker_block_shows_only_its_layer() {
    let creative = Some(PlayerGameMode::Creative);
    for marker in [
        TerrainMarker::Barrier,
        TerrainMarker::Light,
        TerrainMarker::StructureVoid,
    ] {
        assert_eq!(shown_layers(creative, Some(marker)), marker.layer());
    }
    assert_eq!(shown_layers(creative, None), 0);
}

#[test]
fn other_game_modes_show_no_marker_layer() {
    for mode in [
        None,
        Some(PlayerGameMode::Survival),
        Some(PlayerGameMode::Adventure),
        Some(PlayerGameMode::Spectator),
        Some(PlayerGameMode::Unknown),
    ] {
        assert_eq!(shown_layers(mode, Some(TerrainMarker::Light)), 0);
    }
}
