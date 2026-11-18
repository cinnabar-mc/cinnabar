use super::*;
use ui::mod_hud::Anchor;

/// A titled card `width` GUI pixels wide and 22 tall.
fn titled(width: f32) -> Card {
    Card {
        title: "Fixture".into(),
        width,
        ..Default::default()
    }
}

/// Untitled cards keep a thin band and titled cards add their header.
#[test]
fn card_height_follows_its_title_band_unless_the_host_measured_it() {
    let mut card = titled(100.);
    assert_eq!(dimensions(&card), [100., 22.]);
    card.scale = 2.;
    assert_eq!(dimensions(&card), [200., 44.]);
    card.title.clear();
    assert_eq!(dimensions(&card), [200., 8.]);
    card.editor_size = Some([30., 40.]);
    assert_eq!(dimensions(&card), [60., 80.]);
}

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
                ..titled(100.)
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
        (Anchor::Left, [13., 186.]),
        (Anchor::Center, [283., 186.]),
        (Anchor::Right, [527., 186.]),
        (Anchor::BottomLeft, [13., 321.]),
        (Anchor::BottomCenter, [283., 321.]),
        (Anchor::BottomRight, [527., 321.]),
    ] {
        let card = Card {
            anchor,
            offset: [13., 17.],
            ..titled(100.)
        };
        assert_eq!(origin(&card, [640., 360.]), expected, "{anchor:?}");
        assert_eq!(origin(&card, [100., 22.]), [0.; 2]);
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
        ..titled(130.)
    };
    assert_eq!(origin(&card, [320., 160.]), [8., 138.]);
    assert_eq!(origin(&card, [800., 500.]), [8., 176.]);
    assert_eq!(card.position, None);
}

/// Oversized anchored and normalized axes both collapse to zero available travel.
#[test]
fn oversized_and_normalized_placement_keep_their_existing_contracts() {
    let mut card = Card {
        anchor: Anchor::TopRight,
        offset: [-8., 180.],
        ..titled(300.)
    };
    assert_eq!(origin(&card, [220., 120.]), [0., 98.]);
    card.position = Some([0.75, 0.25]);
    assert_eq!(origin(&card, [220., 120.]), [0., 24.5]);
    assert_eq!(origin(&card, [700., 300.]), [300., 69.5]);
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
