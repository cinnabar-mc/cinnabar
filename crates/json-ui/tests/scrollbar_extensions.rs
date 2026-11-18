//! Mod-surface scrollbar extensions: box and track state children, the minimum box
//! fraction, the wheel easing value, mouse auto-hide, and that catalogs without the opt-in
//! never see any of them.

use std::collections::BTreeMap;

use json_ui::scrollbar::{
    AUTO_HIDE_PROPERTY, DEFAULT_STATE, HOVER_STATE, MIN_THUMB_PROPERTY, PRESSED_STATE,
    WHEEL_SECONDS_PROPERTY,
};
use json_ui::{
    Catalog, Context, LaidOut, LayoutEnv, ResolvedControl, TextMeasure, TextureMeta, TextureSource,
    ViewState, hit_regions, layout_with,
};
use serde_json::{Value, json};

struct Blank;
impl TextMeasure for Blank {
    fn extent(&self, _text: &str) -> [f64; 2] {
        [0.0, 0.0]
    }
}
impl TextureSource for Blank {
    fn texture(&self, _path: &str) -> Option<TextureMeta> {
        None
    }
}

fn env() -> LayoutEnv<'static> {
    LayoutEnv {
        text: &Blank,
        textures: &Blank,
    }
}

fn ctrl(name: &str, kind: &str, props: Value, children: Vec<ResolvedControl>) -> ResolvedControl {
    let mut props = props;
    props["anchor_from"] = json!("top_left");
    props["anchor_to"] = json!("top_left");
    let properties: BTreeMap<String, Value> = props
        .as_object()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .collect();
    ResolvedControl {
        name: name.to_owned(),
        control_type: Some(kind.to_owned()),
        base: None,
        unresolved_base: None,
        properties: properties.into(),
        children,
        factory: None,
    }
}

/// The box and track's three state children, filling their parent.
fn states() -> Vec<ResolvedControl> {
    ["idle", "lit", "held"]
        .map(|name| ctrl(name, "image", json!({ "size": ["100%", "100%"] }), vec![]))
        .into()
}

/// The state properties naming [`states`].
fn named_states() -> Value {
    json!({ DEFAULT_STATE: "idle", HOVER_STATE: "lit", PRESSED_STATE: "held" })
}

/// A vertical view over `content` px of content in a 50 px viewport, with a 40 px track.
fn view(extra: Value, content: f64) -> ResolvedControl {
    let mut props = json!({
        "size": [100, 50], "scroll_content": "content", "scroll_view_port": "viewport",
        "scrollbar_track": "track", "scrollbar_box": "box", "scroll_box_and_track_panel": "bars"
    });
    for (key, value) in extra.as_object().into_iter().flatten() {
        props[key] = value.clone();
    }
    let mut track = named_states();
    track["size"] = json!([8, 40]);
    let mut thumb = named_states();
    thumb["size"] = json!([8, 10]);
    thumb["draggable"] = json!("vertical");
    ctrl(
        "view",
        "scroll_view",
        props,
        vec![
            ctrl(
                "viewport",
                "panel",
                json!({ "size": [90, 50] }),
                vec![ctrl(
                    "content",
                    "panel",
                    json!({ "size": [90, content] }),
                    vec![],
                )],
            ),
            ctrl(
                "bars",
                "panel",
                json!({ "size": [8, 40], "offset": [92, 0] }),
                vec![
                    ctrl("track", "scroll_track", track, states()),
                    ctrl("box", "scrollbar_box", thumb, states()),
                ],
            ),
        ],
    )
}

fn find<'a>(node: &'a LaidOut<'a>, path: &[&str]) -> &'a LaidOut<'a> {
    path.iter().fold(node, |node, name| {
        node.children
            .iter()
            .find(|child| child.control.name == *name)
            .unwrap_or_else(|| panic!("missing {name}"))
    })
}

/// Which of `idle`, `lit` and `held` show under the box or track named `role`.
fn shown(root: &ResolvedControl, state: &ViewState, role: &str) -> [bool; 3] {
    let (laid, _) = layout_with(root, [200.0, 200.0], &env(), state);
    ["idle", "lit", "held"].map(|name| find(&laid, &["bars", role, name]).visible)
}

#[test]
fn box_and_track_show_their_hover_and_pressed_children() {
    let root = view(json!({}), 400.0);
    for role in ["box", "track"] {
        let key = format!("/view/bars/{role}");
        assert_eq!(
            shown(&root, &ViewState::default(), role),
            [true, false, false]
        );
        let hovered = ViewState {
            hovered: Some(key.clone()),
            ..ViewState::default()
        };
        assert_eq!(shown(&root, &hovered, role), [false, true, false], "{role}");
        let pressed = ViewState {
            pressed: Some(key),
            ..ViewState::default()
        };
        assert_eq!(shown(&root, &pressed, role), [false, false, true], "{role}");
    }
}

#[test]
fn the_box_minimum_follows_the_declared_fraction() {
    let thumb = |extra: Value| {
        let (_, report) = layout_with(
            &view(extra, 10_000.0),
            [200.0, 200.0],
            &env(),
            &ViewState::default(),
        );
        report.scrolls["/view"].thumb.expect("shown")[3]
    };
    assert_eq!(
        thumb(json!({})),
        4.0,
        "a tenth of the 40 px track by default"
    );
    assert_eq!(thumb(json!({ MIN_THUMB_PROPERTY: 0.5 })), 20.0);
    assert_eq!(
        thumb(json!({ MIN_THUMB_PROPERTY: 4 })),
        40.0,
        "clamped to the track"
    );
}

#[test]
fn the_wheel_easing_is_reported_within_its_bound() {
    let seconds = |extra: Value| {
        let (_, report) = layout_with(
            &view(extra, 400.0),
            [200.0, 200.0],
            &env(),
            &ViewState::default(),
        );
        report.scrolls["/view"].wheel_seconds
    };
    assert_eq!(seconds(json!({})), None);
    assert_eq!(seconds(json!({ WHEEL_SECONDS_PROPERTY: 0 })), Some(0.0));
    assert_eq!(seconds(json!({ WHEEL_SECONDS_PROPERTY: 0.3 })), Some(0.3));
    assert_eq!(seconds(json!({ WHEEL_SECONDS_PROPERTY: 9 })), Some(1.0));
}

#[test]
fn an_auto_hidden_box_shows_when_used_and_fades_when_idle() {
    let root = view(json!({ AUTO_HIDE_PROPERTY: true }), 400.0);
    let mut state = ViewState::default();
    let layout = |state: &ViewState| {
        let (laid, report) = layout_with(&root, [200.0, 200.0], &env(), state);
        let regions = hit_regions(&laid);
        (find(&laid, &["bars", "box"]).visible, report, regions)
    };
    let (visible, report, _) = layout(&state);
    assert!(!visible, "hidden until used");
    assert_eq!(report.scrolls["/view"].bar_visible, Some(false));
    assert!(!state.scrollbar_fading(&report));

    assert!(state.reveal_scrollbar("/view", &report.scrolls["/view"]));
    let (visible, report, _) = layout(&state);
    assert!(visible, "a scroll reveals it");
    assert!(state.scrollbar_fading(&report));
    assert!(state.step_scrolls(&report, 0.5));
    assert_eq!(state.scroll_state["/view"].bar_fade, Some(0.5));
    state.step_scrolls(&report, 0.6);
    let (visible, report, regions) = layout(&state);
    assert!(!visible, "faded out");
    assert!(!state.scrollbar_fading(&report));

    // The pointer over the track brings the box back and holds it while it stays.
    let track = report.scrolls["/view"].track.expect("track");
    let hit = json_ui::hit_test(&regions, [track[0] + 1.0, track[1] + 1.0]).expect("track hit");
    state.hovered = Some(hit.key.clone());
    assert!(state.step_scrolls(&report, 0.1));
    assert!(!state.step_scrolls(&report, 5.0), "held while hovered");
    assert!(!state.scrollbar_fading(&report));
    assert!(layout(&state).0);
    state.hovered = None;
    assert!(state.scrollbar_fading(&report));
}

#[test]
fn a_view_without_auto_hide_ignores_reveals() {
    let root = view(json!({}), 400.0);
    let mut state = ViewState::default();
    let (_, report) = layout_with(&root, [200.0, 200.0], &env(), &state);
    assert!(!state.reveal_scrollbar("/view", &report.scrolls["/view"]));
    assert!(state.scroll_state.is_empty());
}

/// Resolves a one-file catalog, opted into Cinnabar extensions or not.
fn resolve(opted: bool, root: Value) -> ResolvedControl {
    let files = br#"{"ui_defs":["ui/fixture.json"]}"#;
    let document = json!({ "namespace": "fixture", "root": root }).to_string();
    let catalog = Catalog::from_files([
        ("ui/_global_variables.json", &b"{}"[..]),
        ("ui/_ui_defs.json", &files[..]),
        ("ui/fixture.json", document.as_bytes()),
    ])
    .unwrap();
    let catalog = if opted {
        catalog.with_paint_transforms()
    } else {
        catalog
    };
    json_ui::resolve(&catalog, "fixture.root", &Context::default())
        .control
        .expect("resolved")
}

#[test]
fn only_opted_in_catalogs_keep_the_extensions() {
    let mut thumb = named_states();
    thumb["type"] = json!("scrollbar_box");
    let root = json!({
        "type": "scroll_view", MIN_THUMB_PROPERTY: 0.5, WHEEL_SECONDS_PROPERTY: 0.2,
        AUTO_HIDE_PROPERTY: true, "controls": [{ "box": thumb }]
    });
    let extensions = |control: &ResolvedControl| {
        [
            MIN_THUMB_PROPERTY,
            WHEEL_SECONDS_PROPERTY,
            AUTO_HIDE_PROPERTY,
        ]
        .iter()
        .filter(|name| control.properties.contains_key(**name))
        .count()
            + [DEFAULT_STATE, HOVER_STATE, PRESSED_STATE]
                .iter()
                .filter(|name| control.children[0].properties.contains_key(**name))
                .count()
    };
    assert_eq!(extensions(&resolve(true, root.clone())), 6);
    assert_eq!(
        extensions(&resolve(false, root)),
        0,
        "server and pack JSON-UI"
    );
}
