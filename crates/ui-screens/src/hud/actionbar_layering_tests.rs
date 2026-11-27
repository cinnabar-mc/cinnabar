use super::*;
use json_ui::{Draw, LayoutEnv, TextMeasure, TextureMeta, TextureSource, Timed};

struct FixtureResources;

impl TextMeasure for FixtureResources {
    fn extent(&self, text: &str) -> [f64; 2] {
        [
            text.lines()
                .map(|line| line.chars().count())
                .max()
                .unwrap_or(0) as f64
                * 6.0,
            text.lines().count() as f64 * 9.0,
        ]
    }
}

impl TextureSource for FixtureResources {
    fn texture(&self, _: &str) -> Option<TextureMeta> {
        None
    }
}

/// An original carrier with vanilla-style default anchors and a normal actionbar factory.
fn carrier() -> Catalog {
    Catalog::from_files([
        (json_ui::UI_DEFS_PATH, br#"{"ui_defs":["ui/hud_screen.json"]}"#.as_slice()),
        (json_ui::GLOBAL_VARIABLES_PATH, b"{}".as_slice()),
        ("ui/hud_screen.json", br##"{
            "namespace":"hud",
            "hud_screen":{"type":"screen","size":["100%","100%"],"controls":[{"root_panel@hud.root_panel":{}}]},
            "root_panel":{"type":"panel","size":["100%","100%"],"controls":[
                {"hud_actionbar_text_area":{"type":"panel","size":["100%","100%"],"factory":{
                    "name":"hud_actionbar_text_factory","control_ids":{"hud_actionbar_text":"hud.hud_actionbar_text"}
                }}}
            ]},
            "hud_actionbar_text":{"type":"label","text":"$actionbar_text"}
        }"##.as_slice()),
    ]).unwrap()
}

/// A server panel relies on carrier defaults rather than restating its anchors.
fn server_panel() -> Vec<(String, Vec<u8>)> {
    vec![("ui/hud_screen.json".into(), br##"{
        "hud_actionbar_text":{"type":"panel","size":["100%",40],"offset":[0,"50%-100px"],
            "controls":[{"message":{"type":"label","text":"$actionbar_text","localize":false,
                "size":[120,20],"anchor_from":"bottom_right","anchor_to":"bottom_right","offset":[-20,0]
            }}]
        }
    }"##.to_vec())]
}

fn actionbar_draws(catalog: &Catalog, text: &str) -> Vec<json_ui::DrawNode> {
    let context = Context::retail(true);
    let root = json_ui::resolve(catalog, "hud.hud_screen", &context)
        .control
        .expect("HUD screen resolves");
    let data = json_ui::hud_data_source(&HudModel {
        actionbar: Some(Timed {
            text: text.into(),
            born: 0.0,
        }),
        ..Default::default()
    });
    let bound = json_ui::bind_screen(
        &root,
        catalog,
        &context,
        &data,
        &mut json_ui::BindState::new(),
    );
    let resources = FixtureResources;
    let env = LayoutEnv {
        text: &resources,
        textures: &resources,
    };
    json_ui::emit(&json_ui::layout(&bound, [640.0, 360.0], &env), &env)
        .into_iter()
        .filter(|node| matches!(&node.draw, Draw::Text { text: drawn, .. } if drawn == text))
        .collect()
}

fn assert_visible(nodes: &[json_ui::DrawNode]) {
    assert!(
        !nodes.is_empty(),
        "actionbar text must be emitted inside the viewport"
    );
    assert!(
        nodes.iter().any(|node| {
            node.alpha > 0.0
                && node.dest.w > 0.0
                && node.dest.h > 0.0
                && node.dest.x >= 0.0
                && node.dest.y >= 0.0
                && node.dest.x + node.dest.w <= 640.0
                && node.dest.y + node.dest.h <= 360.0
        }),
        "actionbar text lies outside the viewport: {nodes:#?}"
    );
}

#[test]
fn actionbar_layering_server_panel_uses_carrier_defaults() {
    let mut registry = ui_engine::engine::screen::ScreenRegistry::new(Context::retail(true));
    super::register(&mut registry);
    let layered = registry.catalog().layer(&carrier(), &[server_panel()]);
    for text in ["15/30\nFixture ammo", "14/30\nFixture ammo"] {
        assert_visible(&actionbar_draws(&layered.pack, text));
    }
}

#[test]
fn actionbar_layering_without_override_keeps_builtin_presentation() {
    let mut registry = ui_engine::engine::screen::ScreenRegistry::new(Context::retail(true));
    super::register(&mut registry);
    let layered = registry.catalog().layer(&carrier(), &[]);
    let nodes = actionbar_draws(&layered.pack, "Built-in presentation");
    assert_visible(&nodes);
    assert!(
        nodes
            .iter()
            .any(|node| matches!(&node.draw, Draw::Text { shadow: true, .. })),
        "the built-in shadow remains when no server overrides the actionbar"
    );
}

#[test]
fn actionbar_layering_unrelated_layer_before_authoring_preserves_visibility() {
    let mut registry = ui_engine::engine::screen::ScreenRegistry::new(Context::retail(true));
    super::register(&mut registry);
    let layers = [
        vec![(
            "ui/hud_screen.json".into(),
            br#"{"fixture_unrelated":{"type":"panel"}}"#.to_vec(),
        )],
        server_panel(),
    ];
    let layered = registry.catalog().layer(&carrier(), &layers);
    assert_visible(&actionbar_draws(&layered.pack, "First authoring layer"));
}

#[test]
fn actionbar_layering_later_server_layer_preserves_earlier_customization() {
    let mut registry = ui_engine::engine::screen::ScreenRegistry::new(Context::retail(true));
    super::register(&mut registry);
    let layers = [
        vec![(
            "ui/hud_screen.json".into(),
            br##"{
            "hud_actionbar_text":{"color":[0.2,0.8,0.4]}
        }"##
            .to_vec(),
        )],
        server_panel(),
    ];
    let layered = registry.catalog().layer(&carrier(), &layers);
    assert_eq!(
        layered
            .pack
            .lookup("hud", "hud_actionbar_text")
            .unwrap()
            .props["color"],
        serde_json::json!([0.2, 0.8, 0.4])
    );
    assert_visible(&actionbar_draws(&layered.pack, "Layered message"));
}
