//! Gated layouts that splice unchanged subtrees must match a layout placing every control.

use serde_json::{Value, json};

use super::super::{LayoutEnv, MeasureCache, ResolvedControl, TextMeasure, TextureSource};
use crate::sidecar::TextureMeta;
use crate::state::ViewState;

struct Fixed;

impl TextMeasure for Fixed {
    fn extent(&self, text: &str) -> [f64; 2] {
        [text.chars().count() as f64 * 6.0, 9.0]
    }
}

impl TextureSource for Fixed {
    fn texture(&self, _: &str) -> Option<TextureMeta> {
        None
    }
}

fn node(
    name: &str,
    kind: &str,
    properties: Value,
    children: Vec<ResolvedControl>,
) -> ResolvedControl {
    ResolvedControl {
        name: name.into(),
        control_type: Some(kind.into()),
        properties: properties
            .as_object()
            .unwrap()
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
        children,
        base: None,
        unresolved_base: None,
        factory: None,
    }
}

/// A button whose hover look the neutral state hides, labelled `text`.
fn button(name: &str, text: &str) -> ResolvedControl {
    let look = |name: &str, visible: bool| {
        node(
            name,
            "image",
            json!({"size": ["100%", "100%"], "texture": "textures/ui/x", "visible": visible}),
            vec![],
        )
    };
    node(
        name,
        "button",
        json!({
            "size": [100, 20],
            "default_control": "default",
            "hover_control": "hover",
            "pressed_control": "hover",
        }),
        vec![
            look("default", true),
            look("hover", false),
            node("label", "label", json!({"text": text}), vec![]),
        ],
    )
}

/// A titled screen with a button outside and `rows` labelled buttons inside a scroll view.
fn screen(title: &str, rows: &[String]) -> ResolvedControl {
    let content = node(
        "content",
        "stack_panel",
        json!({"size": [100, "100%c"], "orientation": "vertical"}),
        rows.iter()
            .enumerate()
            .map(|(index, text)| button(&format!("row{index}"), text))
            .collect(),
    );
    let view = node(
        "view",
        "scroll_view",
        json!({
            "size": [108, 100], "offset": [0, 40], "anchor_from": "top_left", "anchor_to": "top_left",
            "scroll_content": "content", "scroll_view_port": "viewport", "scrollbar_track": "track",
            "scrollbar_box": "box", "scroll_box_and_track_panel": "bars",
        }),
        vec![
            node(
                "viewport",
                "panel",
                json!({"size": [100, 100], "clips_children": true, "anchor_from": "top_left", "anchor_to": "top_left"}),
                vec![content],
            ),
            // A fixed wrapper holds the bar panel, with the track and box outside it: only the
            // panel's role ties the wrapper to the scroll state.
            node(
                "rail",
                "panel",
                json!({"size": [8, 100], "offset": [100, 0], "anchor_from": "top_left", "anchor_to": "top_left"}),
                vec![node(
                    "bars",
                    "panel",
                    json!({"size": [8, 100]}),
                    vec![node("grip", "label", json!({"text": "|"}), vec![])],
                )],
            ),
            node(
                "track",
                "scroll_track",
                json!({"size": [8, 100], "offset": [100, 0], "anchor_from": "top_left", "anchor_to": "top_left"}),
                vec![],
            ),
            node(
                "box",
                "scrollbar_box",
                json!({"size": [8, 10], "offset": [100, 0], "anchor_from": "top_left", "anchor_to": "top_left", "draggable": "vertical"}),
                vec![node(
                    "art",
                    "image",
                    json!({"texture": "textures/ui/x"}),
                    vec![],
                )],
            ),
        ],
    );
    node(
        "root",
        "panel",
        json!({"size": [200, 200]}),
        vec![
            node(
                "title",
                "label",
                json!({"text": title, "anchor_from": "top_left", "anchor_to": "top_left"}),
                vec![],
            ),
            node(
                "menu",
                "panel",
                json!({"size": [100, 20], "anchor_from": "top_right", "anchor_to": "top_right"}),
                vec![button("close", "x")],
            ),
            view,
        ],
    )
}

/// Draws and hit regions of a gated layout placing every control.
fn placed_everywhere(
    tree: &ResolvedControl,
    state: &ViewState,
) -> (
    Vec<crate::DrawNode>,
    Vec<crate::HitRegion>,
    crate::LayoutReport,
) {
    let env = LayoutEnv {
        text: &Fixed,
        textures: &Fixed,
    };
    let mut cache = MeasureCache::default();
    cache.enter(tree);
    let (laid, report) = super::super::lay_out(tree, [200.0, 200.0], &env, state, true, None);
    cache.leave();
    (
        crate::emit::emit_gated(&laid, &env),
        crate::input::hit_regions(&laid),
        report,
    )
}

fn rows(count: usize, changed: Option<usize>) -> Vec<String> {
    (0..count)
        .map(|index| {
            if changed == Some(index) {
                format!("changed {index}")
            } else {
                format!("row {index}")
            }
        })
        .collect()
}

#[test]
fn spliced_gated_layouts_match_layouts_placing_every_control() {
    let env = LayoutEnv {
        text: &Fixed,
        textures: &Fixed,
    };
    let mut cache = MeasureCache::default();
    let mut tree = screen("title", &rows(30, None));
    let mut placed = Vec::new();
    for (step, (title, count, changed, offset)) in [
        ("title", 30, None, 0.0),
        ("title", 30, Some(2), 0.0),
        ("longer title", 30, Some(2), 0.0),
        ("longer title", 30, Some(3), 45.0),
        ("longer title", 30, Some(3), 45.0),
        ("t", 30, Some(29), 45.0),
        // The content shrinks to fit, hiding the bars, then grows past the view again.
        ("t", 3, None, 45.0),
        ("t", 3, Some(1), 45.0),
        ("t", 30, None, 45.0),
    ]
    .into_iter()
    .enumerate()
    {
        let next = screen(title, &rows(count, changed));
        if step > 0 {
            cache.update_tree(&mut tree, next.clone());
        }
        let state = ViewState {
            scroll: [("/root/view".to_owned(), offset)].into_iter().collect(),
            ..ViewState::default()
        };
        let (output, report) =
            super::super::layout_reusing(&tree, [200.0, 200.0], &env, &state, &mut cache, true);
        let (nodes, hits, cold_report) = placed_everywhere(&next, &state);
        assert_eq!(*output.nodes, *nodes, "step {step}: draws");
        assert_eq!(*output.hits, *hits, "step {step}: hit regions");
        assert_eq!(report, cold_report, "step {step}: report");
        assert_eq!(
            report.scrolls["/root/view"].bar_visible,
            Some(count > 3),
            "step {step}: bars"
        );
        assert!(
            output.nodes.iter().any(|node| !node.gates.is_empty()),
            "step {step}: hover looks emit gated"
        );
        placed.push(cache.placed());
    }
    let all = placed[0];
    // A changed label in view, the title, or nothing places only what changed and its ancestors;
    // a new scroll offset places everything again.
    assert!(placed[1] < all / 3, "{placed:?}");
    assert!(placed[2] < all / 3, "{placed:?}");
    assert!(placed[4] < all / 3, "{placed:?}");
    assert!(placed[5] < all / 3, "{placed:?}");
}
