use super::*;
use ui::mod_hud::Anchor;

/// Authored offsets keep fitting widgets reachable for every supported anchor after a resize.
#[test]
fn anchored_cards_remain_reachable_in_a_smaller_viewport() {
    for anchor in [
        Anchor::TopLeft,
        Anchor::TopCenter,
        Anchor::TopRight,
        Anchor::Left,
        Anchor::Center,
        Anchor::Right,
        Anchor::BottomLeft,
        Anchor::BottomCenter,
        Anchor::BottomRight,
    ] {
        for offset in [[600., 400.], [-600., -400.]] {
            let card = Card {
                anchor,
                offset,
                width: 100.,
                cells: vec![ui::mod_hud::Cell {
                    rect: [0., 0., 100., 60.],
                    ..Default::default()
                }],
                ..Default::default()
            };
            for viewport in [[800., 500.], [220., 120.]] {
                let at = origin(&card, viewport);
                let size = dimensions(&card);
                for axis in 0..2 {
                    assert!(at[axis] >= 0.);
                    assert!(at[axis] + size[axis] <= viewport[axis]);
                }
                assert_eq!(
                    card.offset, offset,
                    "bounds correction must preserve authored offsets"
                );
            }
        }
    }
}

/// All nine anchor references use inward edge distances and signed center offsets.
#[test]
fn all_anchor_references_keep_their_authored_offsets() {
    for (anchor, expected) in [
        (Anchor::TopLeft, [13., 17.]),
        (Anchor::TopCenter, [283., 17.]),
        (Anchor::TopRight, [527., 17.]),
        (Anchor::Left, [13., 167.]),
        (Anchor::Center, [283., 167.]),
        (Anchor::Right, [527., 167.]),
        (Anchor::BottomLeft, [13., 283.]),
        (Anchor::BottomCenter, [283., 283.]),
        (Anchor::BottomRight, [527., 283.]),
    ] {
        let card = Card {
            anchor,
            offset: [13., 17.],
            width: 100.,
            cells: vec![ui::mod_hud::Cell {
                rect: [0., 0., 100., 60.],
                ..Default::default()
            }],
            ..Default::default()
        };
        assert_eq!(origin(&card, [640., 360.]), expected, "{anchor:?}");
        assert_eq!(origin(&card, [100., 60.]), [0.; 2]);
        assert_eq!(origin(&card, [640., 360.]), expected, "restored {anchor:?}");
        assert_eq!(card.offset, [13., 17.]);
        assert_eq!(card.position, None);
    }
}

/// Effective bounds correction does not replace the default layout saved in the card.
#[test]
fn factory_offsets_return_when_the_viewport_grows() {
    let card = Card {
        offset: [8., 176.],
        width: 130.,
        cells: vec![ui::mod_hud::Cell {
            rect: [0., 0., 130., 18.],
            ..Default::default()
        }],
        ..Default::default()
    };
    assert_eq!(origin(&card, [320., 160.]), [8., 142.]);
    assert_eq!(origin(&card, [800., 500.]), [8., 176.]);
    assert_eq!(card.position, None);
}

/// Oversized anchored and normalized axes both collapse to zero available travel.
#[test]
fn oversized_and_normalized_placement_keep_their_existing_contracts() {
    let mut card = Card {
        anchor: Anchor::TopRight,
        offset: [-8., 180.],
        width: 300.,
        cells: vec![ui::mod_hud::Cell {
            rect: [0., 0., 300., 60.],
            ..Default::default()
        }],
        ..Default::default()
    };
    assert_eq!(origin(&card, [220., 120.]), [0., 60.]);
    card.position = Some([0.75, 0.25]);
    assert_eq!(origin(&card, [220., 120.]), [0., 15.]);
    assert_eq!(origin(&card, [700., 300.]), [300., 60.]);
}

/// An inventory has nine columns and retains all 36 slots, including empty ones.
#[test]
fn inventory_grid_geometry_preserves_slot_count() {
    let card = Card {
        id: "inventory".into(),
        width: 180.,
        row_height: 20.,
        icon_size: 16.,
        row_layout: RowLayout::Grid,
        rows: (0..36)
            .map(|_| ui::mod_hud::Row {
                label: String::new(),
                value: String::new(),
                item: None,
                effect_id: None,
                metadata: 0,
                progress: None,
                color: [1.; 4],
                localized: false,
                formatted: false,
            })
            .collect(),
        ..Default::default()
    };
    assert_eq!(dimensions(&card), [180., 84.]);
    let mut count = 0;
    let _ = grid(&card, &mut count, 0.);
    assert_eq!(count, 36);
}

/// Positive edge offsets remain inward after a viewport resize.
#[test]
fn anchored_edge_offset_survives_viewport_change() {
    let card = ui::mod_hud::Card {
        anchor: ui::mod_hud::Anchor::BottomRight,
        offset: [13., 17.],
        ..Default::default()
    };
    let size = super::dimensions(&card);
    for viewport in [[640., 360.], [960., 540.]] {
        let at = super::origin(&card, viewport);
        for axis in 0..2 {
            assert!(
                (viewport[axis] - at[axis] - size[axis] - f64::from(card.offset[axis])).abs()
                    < 0.001
            );
        }
    }
}
