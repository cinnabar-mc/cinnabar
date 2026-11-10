//! Gated layouts that splice unchanged subtrees must match a layout placing every control.

use serde_json::{Value, json};

use super::super::{LayoutEnv, MeasureCache, ResolvedControl, TextMeasure, TextureSource};
use crate::sidecar::TextureMeta;
use crate::state::{ScrollRetained, ViewState};

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

/// The screen's tree-borne variations.
#[derive(Clone, Debug)]
struct Shape {
    title: &'static str,
    rows: usize,
    changed: Option<usize>,
    /// The control the view scrolls: its buttons (`content`) or labels in a wrapper (`alt`).
    scrolled: &'static str,
    touch_mode: bool,
    /// Whether the view names its bar panel.
    panel: bool,
    /// Whether a fixed wrapper holds the bar panel.
    rail: bool,
    /// Whether a short clipping wrapper holds a bare scroll box image.
    sleeve: bool,
}

impl Default for Shape {
    fn default() -> Self {
        Self {
            title: "title",
            rows: 30,
            changed: None,
            scrolled: "content",
            touch_mode: false,
            panel: true,
            rail: true,
            sleeve: false,
        }
    }
}

/// The view state's variations: the view's offset and its touch box fade.
#[derive(Clone, Copy, Debug, Default)]
struct Scroll {
    offset: f64,
    fade: Option<f32>,
}

impl Scroll {
    fn state(self) -> ViewState {
        ViewState {
            scroll: [("/root/view".to_owned(), self.offset)]
                .into_iter()
                .collect(),
            scroll_state: [(
                "/root/view".to_owned(),
                ScrollRetained {
                    bar_fade: self.fade,
                    ..ScrollRetained::default()
                },
            )]
            .into_iter()
            .collect(),
            ..ViewState::default()
        }
    }
}

fn anchored(mut properties: Value) -> Value {
    properties["anchor_from"] = json!("top_left");
    properties["anchor_to"] = json!("top_left");
    properties
}

/// A titled screen with a button outside and a scroll view of `shape.rows` labelled buttons
/// beside a wrapper of labels, with a track, a box and a bar panel arranged per `shape`.
fn screen(shape: &Shape) -> ResolvedControl {
    let content = node(
        "content",
        "stack_panel",
        anchored(json!({"size": [100, "100%c"], "orientation": "vertical"})),
        (0..shape.rows)
            .map(|index| {
                let text = if shape.changed == Some(index) {
                    format!("changed {index}")
                } else {
                    format!("row {index}")
                };
                button(&format!("row{index}"), &text)
            })
            .collect(),
    );
    let shelf = node(
        "shelf",
        "panel",
        anchored(json!({"size": [100, 100]})),
        vec![node(
            "alt",
            "stack_panel",
            anchored(json!({"size": [100, "100%c"], "orientation": "vertical"})),
            (0..30)
                .map(|index| {
                    node(
                        &format!("alt{index}"),
                        "label",
                        json!({"size": [100, 20], "text": format!("alt {index}")}),
                        vec![],
                    )
                })
                .collect(),
        )],
    );
    let bars = node(
        "bars",
        "panel",
        anchored(json!({"size": [8, 100], "offset": [100, 0]})),
        vec![node("grip", "label", json!({"text": "|"}), vec![])],
    );
    let bars = if shape.rail {
        node(
            "rail",
            "panel",
            anchored(json!({"size": [108, 100]})),
            vec![bars],
        )
    } else {
        bars
    };
    let thumb = if shape.sleeve {
        // A bare thumb image, which culls wholly outside its wrapper's clip.
        node(
            "sleeve",
            "panel",
            anchored(json!({"size": [8, 20], "offset": [100, 0], "clips_children": true})),
            vec![node(
                "box",
                "image",
                anchored(
                    json!({"size": [8, 10], "draggable": "vertical", "texture": "textures/ui/x"}),
                ),
                vec![],
            )],
        )
    } else {
        node(
            "box",
            "scrollbar_box",
            anchored(json!({"size": [8, 10], "offset": [100, 0], "draggable": "vertical"})),
            vec![node(
                "art",
                "image",
                json!({"texture": "textures/ui/x"}),
                vec![],
            )],
        )
    };
    let mut view = json!({
        "size": [108, 100], "offset": [0, 40],
        "scroll_content": shape.scrolled, "scroll_view_port": "viewport",
        "scrollbar_track": "track", "scrollbar_box": "box", "touch_mode": shape.touch_mode,
    });
    if shape.panel {
        view["scroll_box_and_track_panel"] = json!("bars");
    }
    let view = node(
        "view",
        "scroll_view",
        anchored(view),
        vec![
            node(
                "viewport",
                "panel",
                anchored(json!({"size": [100, 100], "clips_children": true})),
                vec![content, shelf],
            ),
            bars,
            node(
                "track",
                "scroll_track",
                anchored(json!({"size": [8, 100], "offset": [100, 0]})),
                vec![],
            ),
            thumb,
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
                anchored(json!({"text": shape.title})),
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

/// Draws, hit regions and report of a gated layout placing every control.
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

/// Lays out each step over one retained cache, asserting each spliced layout equals one
/// placing every control; returns how many controls each placed.
fn replay(steps: &[(Shape, Scroll)]) -> Vec<usize> {
    let env = LayoutEnv {
        text: &Fixed,
        textures: &Fixed,
    };
    let mut cache = MeasureCache::default();
    let mut tree = screen(&steps[0].0);
    let mut placed = Vec::new();
    for (step, (shape, scroll)) in steps.iter().enumerate() {
        let next = screen(shape);
        if step > 0 {
            cache.update_tree(&mut tree, next.clone());
        }
        let state = scroll.state();
        let (output, report) =
            super::super::layout_reusing(&tree, [200.0, 200.0], &env, &state, &mut cache, true);
        let (nodes, hits, cold_report) = placed_everywhere(&next, &state);
        let context = format!("step {step}: {shape:?} {scroll:?}");
        assert_eq!(*output.nodes, *nodes, "{context}: draws");
        assert_eq!(*output.hits, *hits, "{context}: hit regions");
        assert_eq!(report, cold_report, "{context}: report");
        placed.push(cache.placed());
    }
    placed
}

#[test]
fn spliced_gated_layouts_match_layouts_placing_every_control() {
    let base = Shape::default();
    let at = |offset| Scroll { offset, fade: None };
    let with = |change: &dyn Fn(&mut Shape)| {
        let mut shape = base.clone();
        change(&mut shape);
        shape
    };
    let placed = replay(&[
        (base.clone(), at(0.0)),
        (with(&|shape| shape.changed = Some(2)), at(0.0)),
        (
            with(&|shape| {
                shape.changed = Some(2);
                shape.title = "longer title";
            }),
            at(0.0),
        ),
        (with(&|shape| shape.changed = Some(3)), at(45.0)),
        (with(&|shape| shape.changed = Some(3)), at(45.0)),
        (with(&|shape| shape.changed = Some(29)), at(45.0)),
        // The content shrinks to fit, hiding the bars, then grows past the view again.
        (with(&|shape| shape.rows = 3), at(45.0)),
        (
            with(&|shape| {
                shape.rows = 3;
                shape.changed = Some(1);
            }),
            at(45.0),
        ),
        (base.clone(), at(45.0)),
        (with(&|shape| shape.changed = Some(5)), at(45.0)),
        // The view scrolls labels inside an unchanged wrapper instead, then its rows again.
        (with(&|shape| shape.scrolled = "alt"), at(45.0)),
        (
            with(&|shape| {
                shape.scrolled = "alt";
                shape.changed = Some(4);
            }),
            at(45.0),
        ),
        (base.clone(), at(45.0)),
    ]);
    let all = placed[0];
    // A changed label in view, the title, or nothing places only what changed, its
    // ancestors and the scroll view's bars; a new scroll offset places everything again.
    for step in [1, 2, 4, 5] {
        assert!(placed[step] < all / 3, "{placed:?}");
    }
}

// A box clipped by its own short wrapper is culled at the end of the track; growing the
// content at the same offset moves it back into the wrapper.
#[test]
fn a_clipped_box_shows_again_when_the_content_grows() {
    let shape = |rows, changed| Shape {
        rows,
        changed,
        panel: false,
        sleeve: true,
        ..Shape::default()
    };
    let at = Scroll {
        offset: 100.0,
        fade: None,
    };
    replay(&[
        (shape(10, None), at),
        (shape(10, Some(1)), at),
        (shape(50, Some(1)), at),
    ]);
}

// A touch box's fade applies only in touch mode.
#[test]
fn a_box_fade_follows_touch_mode() {
    let shape = |touch_mode, changed| Shape {
        touch_mode,
        changed,
        ..Shape::default()
    };
    let at = Scroll {
        offset: 45.0,
        fade: Some(0.5),
    };
    replay(&[
        (shape(true, None), at),
        (shape(true, Some(1)), at),
        (shape(false, Some(1)), at),
        (shape(true, Some(1)), at),
    ]);
}

/// A deterministic xorshift stream.
struct Stream(u64);

impl Stream {
    fn below(&mut self, bound: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % bound as u64) as usize
    }

    fn pick<T: Copy>(&mut self, options: &[T]) -> T {
        options[self.below(options.len())]
    }
}

// Random walks over scroll state and structure: each step changes one or two things.
#[test]
fn spliced_gated_layouts_match_over_random_scroll_changes() {
    for seed in 1..=24u64 {
        let mut stream = Stream(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let mut shape = Shape::default();
        let mut scroll = Scroll::default();
        let mut steps = vec![(shape.clone(), scroll)];
        for _ in 0..40 {
            for _ in 0..=stream.below(2) {
                match stream.below(9) {
                    0 => scroll.offset = stream.pick(&[0.0, 17.5, 45.0, 100.0, 300.0]),
                    1 => scroll.fade = stream.pick(&[None, Some(0.0), Some(0.5)]),
                    2 => shape.rows = stream.pick(&[2, 3, 10, 30, 50]),
                    3 => shape.touch_mode = !shape.touch_mode,
                    4 => shape.sleeve = !shape.sleeve,
                    5 => shape.rail = !shape.rail,
                    6 => shape.panel = !shape.panel,
                    7 => shape.scrolled = stream.pick(&["content", "alt"]),
                    _ => shape.changed = Some(stream.below(shape.rows)),
                }
            }
            // Often nothing but a label changes, so later steps have records to splice.
            if stream.below(2) == 0 {
                steps.push((shape.clone(), scroll));
            }
            shape.changed = Some(stream.below(shape.rows));
            steps.push((shape.clone(), scroll));
        }
        replay(&steps);
    }
}
