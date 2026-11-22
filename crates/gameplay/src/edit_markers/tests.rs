use super::{markers_by, resolve};
use assets::NetworkIdMode;
use client_world::game_mode_capabilities::{EditMarker, EditMarkers};
use sim::PaletteWorld;
use world::{BlockUpdate, ChunkStore, SubChunkKey};

/// Builds a column reader from marker heights; all other heights are ordinary blocks.
fn column(heights: &[(i32, EditMarker)]) -> impl FnMut([i32; 3]) -> Result<Option<EditMarker>, ()> {
    let heights = heights.to_vec();
    move |position: [i32; 3]| {
        Ok(heights
            .iter()
            .find(|(height, _)| *height == position[1])
            .map(|(_, marker)| *marker))
    }
}

#[test]
fn no_marker_leaves_the_ordinary_edit_rule() {
    assert_eq!(
        markers_by([0, 10, 0], -64, column(&[])),
        EditMarkers::default()
    );
}

#[test]
fn the_highest_marker_at_or_below_the_cell_governs() {
    assert_eq!(
        markers_by(
            [0, 10, 0],
            -64,
            column(&[(2, EditMarker::Deny), (5, EditMarker::Allow)]),
        )
        .column,
        Some(EditMarker::Allow)
    );
    assert_eq!(
        markers_by(
            [0, 10, 0],
            -64,
            column(&[(2, EditMarker::Allow), (5, EditMarker::Deny)]),
        )
        .column,
        Some(EditMarker::Deny)
    );
}

#[test]
fn markers_above_the_cell_and_other_columns_do_not_govern() {
    assert_eq!(
        markers_by([0, 10, 0], -64, column(&[(11, EditMarker::Deny)])).column,
        None
    );
    let mut in_column = column(&[(3, EditMarker::Deny)]);
    let mut other_column = |position: [i32; 3]| {
        if position[0] == 0 {
            in_column(position)
        } else {
            Ok(Some(EditMarker::Allow))
        }
    };
    assert_eq!(
        markers_by([0, 10, 0], -64, &mut other_column).column,
        Some(EditMarker::Deny),
        "a marker in another column must not govern this one"
    );
}

#[test]
fn a_marker_target_governs_its_own_cell() {
    let markers = markers_by(
        [0, 10, 0],
        -64,
        column(&[(10, EditMarker::Deny), (5, EditMarker::Allow)]),
    );
    assert!(markers.target_is_marker);
    assert_eq!(markers.column, Some(EditMarker::Deny));
}

#[test]
fn scanning_includes_the_lowest_y_and_stops_at_the_first_unreadable_cell() {
    assert_eq!(
        markers_by([0, 10, 0], -64, column(&[(-64, EditMarker::Allow)]),).column,
        Some(EditMarker::Allow)
    );
    let mut unreadable_below = |position: [i32; 3]| {
        if position[1] == 8 {
            Err(())
        } else if position[1] == 3 {
            Ok(Some(EditMarker::Deny))
        } else {
            Ok(None)
        }
    };
    assert_eq!(
        markers_by([0, 10, 0], -64, &mut unreadable_below).column,
        None,
        "a marker below unreadable data must not be guessed"
    );
    let mut unreadable_above = |position: [i32; 3]| {
        if position[1] == 8 {
            Err(())
        } else if position[1] == 9 {
            Ok(Some(EditMarker::Allow))
        } else {
            Ok(None)
        }
    };
    assert_eq!(
        markers_by([0, 10, 0], -64, &mut unreadable_above).column,
        Some(EditMarker::Allow),
        "a marker found before the unreadable cell remains authoritative"
    );
}

/// Builds the pinned block registry with air and both markers in one loaded sub-chunk.
fn fixture() -> (
    ChunkStore,
    crate::movement::PhysicsCollisionRegistries,
    NetworkIdMode,
) {
    let breg = assets::pinned_block_registry_bytes();
    let version = assets::active_content_registry_protocol();
    let records = assets::read_registry_for_protocol(breg, version).unwrap();
    let collisions = crate::movement::PhysicsCollisionRegistries::from_assets(
        breg,
        &records,
        include_bytes!("../../../assets/data/block-physics-v2225.bin"),
        version,
    )
    .unwrap();
    let mode = NetworkIdMode::Sequential;
    let air = collisions
        .block_default_runtime_id(mode, assets::facts::block::AIR_IDENTIFIER)
        .expect("pinned air block");
    let allow = collisions
        .block_default_runtime_id(mode, assets::facts::ALLOW_BLOCK_IDENTIFIER)
        .expect("pinned allow block");
    let deny = collisions
        .block_default_runtime_id(mode, assets::facts::DENY_BLOCK_IDENTIFIER)
        .expect("pinned deny block");
    let mut store = ChunkStore::new();
    let key = SubChunkKey::new(0, 0, 0, 0);
    store.mark_sub_chunk_loaded(key).unwrap();
    for (y, block) in [(2, allow), (4, deny)] {
        store
            .update_block(key, BlockUpdate::new(8, y, 8, 0, block), air)
            .unwrap();
    }
    (store, collisions, mode)
}

#[test]
fn admitted_marker_states_are_identified_and_scanned() {
    let (store, collisions, mode) = fixture();
    let world = PaletteWorld::new(&store, collisions.registry(mode), 0, Default::default());
    let markers = resolve(&collisions, mode, &world, [8, 10, 8], 0);
    assert_eq!(
        markers,
        EditMarkers {
            column: Some(EditMarker::Deny),
            target_is_marker: false,
        }
    );
    let target = resolve(&collisions, mode, &world, [8, 4, 8], 0);
    assert!(target.target_is_marker);
    assert_eq!(
        target.column,
        Some(EditMarker::Deny),
        "the deny target governs its own cell above the allow block"
    );
}
