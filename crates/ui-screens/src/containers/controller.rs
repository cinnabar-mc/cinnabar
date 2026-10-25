//! Original controller pointer artwork and authored focus admission.

use json_ui::{Draw, DrawNode, HitRegion, RectOut};

/// Keeps controller selection subordinate to text and the screen's authored focus rules.
pub(super) fn focus(hits: &[HitRegion], text_owned: bool, key: Option<&str>) -> Option<String> {
    if text_owned {
        return None;
    }
    let key = key?;
    json_ui::focus_order(hits)
        .into_iter()
        .find(|hit| hit.key == key)
        .map(|hit| hit.key.clone())
}

/// A high-contrast, original cross centered on the same GUI point used for dispatch.
pub(super) fn cursor(point: Option<[f32; 2]>) -> Vec<DrawNode> {
    let Some([x, y]) = point.filter(|point| point.iter().all(|value| value.is_finite())) else {
        return Vec::new();
    };
    let (x, y) = (f64::from(x), f64::from(y));
    let clip = RectOut {
        x: x - 4.0,
        y: y - 4.0,
        w: 9.0,
        h: 9.0,
    };
    [
        (
            RectOut {
                x: x - 4.0,
                y: y - 1.0,
                w: 9.0,
                h: 3.0,
            },
            [0, 0, 0, 255],
        ),
        (
            RectOut {
                x: x - 1.0,
                y: y - 4.0,
                w: 3.0,
                h: 9.0,
            },
            [0, 0, 0, 255],
        ),
        (
            RectOut {
                x: x - 3.0,
                y,
                w: 7.0,
                h: 1.0,
            },
            [255; 4],
        ),
        (
            RectOut {
                x,
                y: y - 3.0,
                w: 1.0,
                h: 7.0,
            },
            [255; 4],
        ),
    ]
    .into_iter()
    .map(|(dest, color)| DrawNode {
        name: "controller_cursor".to_owned(),
        key: String::new(),
        dest,
        clip,
        layer: i32::MAX,
        alpha: 1.0,
        paint_region: None,
        anim: None,
        draw: Draw::Solid { color },
        gates: Vec::new(),
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A focused item region without any installed UI carrier.
    fn slot() -> HitRegion {
        HitRegion {
            key: "inventory/slot".into(),
            name: "slot".into(),
            kind: json_ui::HitKind::Button,
            rect: RectOut {
                x: 0.0,
                y: 0.0,
                w: 18.0,
                h: 18.0,
            },
            clip: RectOut {
                x: 0.0,
                y: 0.0,
                w: 3.0,
                h: 18.0,
            },
            layer: 0,
            order: 0,
            pressed: None,
            control_name: None,
            collection_index: Some(0),
            collection: Some("inventory_items".into()),
            enabled: true,
            checked: None,
            max_length: None,
            group_index: None,
            renderer: None,
            drag_axes: [false; 2],
            sound: None,
            input: Default::default(),
            focus: Some(json_ui::FocusMeta {
                enabled: true,
                precedence: 0,
                wrap: false,
                magnet: false,
                identifier: "slot".into(),
                reset_on_focus_lost: true,
                change: Default::default(),
                mapping: Vec::new(),
                containers: Vec::new(),
            }),
            collections: Vec::new(),
            widget: Default::default(),
            modal_root: None,
            transform: None,
        }
    }

    #[test]
    fn authored_focus_is_visible_but_never_replaces_a_text_owner() {
        let mut hits = [slot()];
        let key = hits[0].key.clone();
        assert_eq!(focus(&hits, false, Some(&key)), Some(key.clone()));
        assert_eq!(focus(&hits, true, Some(&key)), None);
        hits[0].enabled = false;
        assert_eq!(focus(&hits, false, Some(&key)), None);
        hits[0].enabled = true;
        hits[0].focus.as_mut().unwrap().enabled = false;
        assert_eq!(focus(&hits, false, Some(&key)), None);
    }

    #[test]
    fn region_mapping_does_not_require_an_unclipped_center_point() {
        use crate::{container_actions::InventoryCellHit, containers::ScreenLayout};
        let region = slot();
        assert!(!region.contains([9.0, 9.0]));
        assert_eq!(
            ScreenLayout::Personal { book: false }.region_hit(&region),
            Some(InventoryCellHit::Player(protocol::HOTBAR_SLOT_COUNT)),
        );
    }

    #[test]
    fn absent_or_nonfinite_pointer_paints_nothing() {
        assert!(cursor(None).is_empty());
        assert!(cursor(Some([f32::NAN, 2.0])).is_empty());
        assert!(cursor(Some([2.0, f32::INFINITY])).is_empty());
    }

    #[test]
    fn pointer_is_centered_on_the_effective_gui_position_with_contrasting_ink() {
        let nodes = cursor(Some([40.0, 30.0]));
        assert!(nodes.iter().all(|node| {
            node.dest.x <= 40.0
                && node.dest.x + node.dest.w > 40.0
                && node.dest.y <= 30.0
                && node.dest.y + node.dest.h > 30.0
        }));
        assert!(nodes.iter().any(|node| matches!(
            node.draw,
            Draw::Solid {
                color: [0, 0, 0, 255]
            }
        )));
        assert!(nodes.iter().any(|node| matches!(
            node.draw,
            Draw::Solid {
                color: [255, 255, 255, 255]
            }
        )));
    }

    #[test]
    fn text_owner_and_missing_authored_target_do_not_acquire_controller_focus() {
        assert_eq!(focus(&[], true, Some("slot")), None);
        assert_eq!(focus(&[], false, Some("slot")), None);
    }
}
