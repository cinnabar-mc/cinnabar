//! HUD surfaces paint through the production engine without game carriers or a window.

use super::*;
use json_ui::{Context, Scalar};
use serde_json::{Value, json};
use std::sync::Arc;
use ui::{UiNode, mod_panel::Surface};
use ui_engine::{
    engine::screen::ScreenRegistry,
    sources::{ImageWork, RemoteImage, RemoteSource},
    text_metrics::TextMetrics,
};

const OWNER: PanelUiOwner = PanelUiOwner::Environment(0);
const OTHER: PanelUiOwner = PanelUiOwner::Environment(1);

struct NoImages;
impl RemoteSource for NoImages {
    /// Test surfaces do not download artwork.
    fn state(&self, _: &str) -> RemoteImage {
        RemoteImage::Failed
    }
}

/// The real engine with an empty atlas and a small redistributable font fixture.
pub(crate) struct Harness {
    engine: FormEngine,
    font: Arc<assets::RuntimeFontCatalog>,
    layouts: ui::TextLayoutCache,
    pub nodes: Vec<UiNode>,
    pub icon: Option<IconRef>,
}

impl Harness {
    pub fn new() -> Self {
        static REGISTRY: std::sync::OnceLock<ScreenRegistry> = std::sync::OnceLock::new();
        let bytes = assets::encode_ui_catalog([1; 32], &[], &[], &[], &[]).unwrap();
        Self {
            engine: FormEngine::new(
                Arc::new(assets::RuntimeUiAssets::decode(&bytes).unwrap()),
                Catalog::default(),
                0,
                REGISTRY.get_or_init(|| ScreenRegistry::new(Context::default())),
                Arc::new(NoImages),
                ImageWork::inline(),
            ),
            font: ui_engine::test_support::fixture_font(),
            layouts: ui::TextLayoutCache::new(1, 1024 * 1024),
            nodes: Vec::new(),
            icon: None,
        }
    }

    /// Paints the gameplay layers, or the open panel, at a 1280 by 720 window and GUI scale 2.
    pub fn paint(&mut self, ui: &mut ModUi, panel: bool) {
        self.nodes.clear();
        let icon = self.icon;
        let mut next = 1;
        let mut frame = PaintFrame {
            layouts: &mut self.layouts,
            font: &self.font,
            metrics: TextMetrics::for_viewport(
                [1280, 720],
                ui::DpiScale::new(1.).unwrap(),
                Some(2),
            ),
            solid_page: 0,
            safe_area: ui::SafeArea::ZERO,
            content: [1280., 720.],
            translate: &|_| None,
            language: [0; 3],
            now: 0.,
            nodes: &mut self.nodes,
            next: &mut next,
            renderers: &mut [],
        };
        if panel {
            ui.paint_mod_panel_with_icons(Some(&self.engine), &mut frame, &|_, _| icon);
        } else {
            ui.paint_hud_surfaces(Some(&self.engine), &mut frame, &|_, _| icon);
        }
    }

    /// Painted rectangles as `[left, top, right, bottom]`, in logical pixels.
    pub fn painted(&self) -> Vec<[f32; 4]> {
        let list = ui::RetainedDraw::build(
            &self.nodes,
            ui_engine::engine::rect(0., 0., 1280., 720.).unwrap(),
            ui::UiScale::default(),
            ui::SafeArea::ZERO,
            ui::TextEffects::default(),
        )
        .unwrap()
        .draw_list()
        .clone();
        list.painted_bounds()
            .map(|bounds| {
                vec![[
                    bounds.min().x(),
                    bounds.min().y(),
                    bounds.max().x(),
                    bounds.max().y(),
                ]]
            })
            .unwrap_or_default()
    }
}

/// A filled 20 by 10 rectangle at `offset`, optionally bound.
fn rect(offset: [f64; 2], bindings: Value) -> Value {
    json!({"type":"custom","renderer":"cinnabar_rounded_rectangle","size":[20,10],
        "offset":offset,"anchor_from":"top_left","anchor_to":"top_left","color":[1,0,0,1],
        "bindings":bindings})
}

/// A HUD surface with three cards: `#a` moves the first, `#b` the second, and the third is
/// an item icon. A world label follows `#tag`.
fn surface(bindings: Value) -> Arc<Surface> {
    Arc::new(
        serde_json::from_value(json!({
            "screen":"hud.root",
            "document":json!({"namespace":"hud",
                "root":{"type":"panel","size":["100%","100%"],"controls":[
                    {"a":rect([10.,10.],json!([{"binding_name":"#a","binding_name_override":"#offset"}]))},
                    {"b":rect([10.,40.],json!([{"binding_name":"#b","binding_name_override":"#alpha"}]))},
                    {"icon":{"type":"custom","renderer":"inventory_item_renderer","size":[16,16],
                        "anchor_from":"top_left","anchor_to":"top_left","offset":[60,60],
                        "bindings":[{"binding_name":"#helmet","binding_name_override":"#item_renderer_data"}]}},
                    {"tag":{"type":"panel","size":[0,0],"anchor_from":"top_left","anchor_to":"top_left",
                        "bindings":[{"binding_name":"#tag","binding_name_override":"#offset"},
                            {"binding_name":"#tag_visible","binding_name_override":"#visible"}],
                        "controls":[{"box":rect([-10.,-5.],json!([]))}]}}
                ]},
                "card":{"type":"panel","size":[40,20],"controls":[{"fill":rect([0.,0.],json!([]))}]}
            }).to_string(),
            "bindings":bindings,
        }))
        .unwrap(),
    )
}

/// Publishes `surface` for `owner` at `revision` with full actor authority.
fn publish(ui: &mut ModUi, layers: &[(PanelUiOwner, u64, &Arc<Surface>)]) {
    ui.set_hud_surfaces(
        layers
            .iter()
            .map(|(owner, revision, surface)| PublishedHudSurface {
                owner: *owner,
                revision: *revision,
                surface,
                authority: ActorAuthority {
                    players: true,
                    entities: true,
                },
            }),
    );
}

fn bindings() -> Value {
    json!({"#a":[10,10],"#b":1,"#helmet":{"item":{"identifier":"minecraft:iron_helmet"}},
        "#tag":{"project":{"world":[0,0,-10]}}})
}

/// A camera at the origin looking down negative Z with a 16:9 perspective.
fn camera() -> [f32; 16] {
    glam::camera::rh::proj::directx::perspective(1.0, 16. / 9., 0.1, 1000.).to_cols_array()
}

#[test]
fn each_top_level_control_paints_once_and_steady_frames_replay() {
    let mut ui = ModUi::default();
    let mut harness = Harness::new();
    let first = surface(bindings());
    publish(&mut ui, &[(OWNER, 1, &first)]);
    harness.paint(&mut ui, false);
    ui.set_hud_projection(Some(camera()), &|_| None);
    harness.paint(&mut ui, false);
    assert_eq!(ui.hud_surface_paints(OWNER), [1, 1, 1, 1]);
    assert!(!harness.painted().is_empty());
    let drawn = harness.nodes.len();
    for _ in 0..3 {
        ui.set_hud_projection(Some(camera()), &|_| None);
        harness.paint(&mut ui, false);
    }
    assert_eq!(
        ui.hud_surface_paints(OWNER),
        [1, 1, 1, 1],
        "steady frames replay"
    );
    assert_eq!(harness.nodes.len(), drawn);
}

#[test]
fn a_binding_change_repaints_only_the_controls_that_read_it() {
    let mut ui = ModUi::default();
    let mut harness = Harness::new();
    publish(&mut ui, &[(OWNER, 1, &surface(bindings()))]);
    harness.paint(&mut ui, false);
    let mut changed = bindings();
    changed["#b"] = json!(0.5);
    publish(&mut ui, &[(OWNER, 2, &surface(changed.clone()))]);
    harness.paint(&mut ui, false);
    assert_eq!(
        ui.hud_surface_paints(OWNER),
        [1, 2, 1, 0],
        "the hidden projected label is never painted"
    );
    publish(&mut ui, &[(OWNER, 2, &surface(changed))]);
    harness.paint(&mut ui, false);
    assert_eq!(
        ui.hud_surface_paints(OWNER),
        [1, 2, 1, 0],
        "an unchanged revision keeps everything"
    );
}

#[test]
fn item_values_resolve_to_icon_indices_and_unknown_items_draw_nothing() {
    let mut ui = ModUi::default();
    let mut harness = Harness::new();
    publish(&mut ui, &[(OWNER, 1, &surface(bindings()))]);
    harness.paint(&mut ui, false);
    assert_eq!(
        ui.hud_surface_bindings(OWNER).unwrap()["#helmet"],
        Scalar::Json(Value::Null)
    );
    harness.icon = Some(IconRef {
        page: 0,
        uv: [0, 0, 16, 16],
        glint: false,
        color_mask: None,
    });
    harness.paint(&mut ui, false);
    assert_eq!(
        ui.hud_surface_bindings(OWNER).unwrap()["#helmet"],
        Scalar::Num(0.)
    );
    assert_eq!(ui.hud_surface_paints(OWNER)[2], 2, "the icon part repaints");
}

#[test]
fn projected_values_follow_the_camera_and_hide_off_screen_or_behind() {
    let mut ui = ModUi::default();
    let mut harness = Harness::new();
    publish(&mut ui, &[(OWNER, 1, &surface(bindings()))]);
    harness.paint(&mut ui, false);
    let resolved = |ui: &ModUi| {
        let values = ui.hud_surface_bindings(OWNER).unwrap();
        (values["#tag"].clone(), values["#tag_visible"].clone())
    };
    ui.set_hud_projection(None, &|_| None);
    assert_eq!(resolved(&ui).1, Scalar::Bool(false), "no camera hides");
    ui.set_hud_projection(Some(camera()), &|_| None);
    assert_eq!(
        resolved(&ui),
        (Scalar::Json(json!([320., 180.])), Scalar::Bool(true))
    );
    harness.paint(&mut ui, false);
    let painted = harness.painted();
    let mut moved = bindings();
    moved["#tag"] = json!({"project":{"world":[100,0,-10]}});
    publish(&mut ui, &[(OWNER, 2, &surface(moved))]);
    ui.set_hud_projection(Some(camera()), &|_| None);
    assert_eq!(resolved(&ui).1, Scalar::Bool(false), "off the root hides");
    let mut actor = bindings();
    actor["#tag"] = json!({"project":{"actor":9,"offset":[0,1,0]}});
    publish(&mut ui, &[(OWNER, 3, &surface(actor))]);
    ui.set_hud_projection(Some(camera()), &|id| {
        (id == 9).then_some(ProjectedActor {
            feet: [0., -1., -10.],
            player: false,
        })
    });
    assert_eq!(resolved(&ui).1, Scalar::Bool(true));
    harness.paint(&mut ui, false);
    assert_eq!(
        ui.hud_surface_paints(OWNER)[3],
        1,
        "a projected label moves its cached artwork instead of repainting"
    );
    assert_eq!(harness.painted(), painted);
    assert_eq!(ui.hud_projected_actors().collect::<Vec<_>>(), [9]);
}

/// Publication hides stale artwork even before the next camera/actor projection pass.
#[test]
fn changed_projections_wait_for_projection_before_painting() {
    for split in [false, true] {
        let mut authored = (*surface(bindings())).clone();
        let mut document: Value = serde_json::from_str(&authored.document).unwrap();
        let tag = document["root"]["controls"][3].clone();
        document["root"]["controls"] = json!([tag]);
        if split {
            document["root"]["controls"]
                .as_array_mut()
                .unwrap()
                .push(json!({"empty":{"type":"panel","size":[0,0]}}));
        }
        authored.document = document.to_string();
        let mut ui = ModUi::default();
        let mut harness = Harness::new();
        publish(&mut ui, &[(OWNER, 1, &Arc::new(authored.clone()))]);
        harness.paint(&mut ui, false);
        assert!(
            harness.painted().is_empty(),
            "initial publication has no camera"
        );
        ui.set_hud_projection(Some(camera()), &|_| None);
        harness.paint(&mut ui, false);
        let shown = harness.painted();
        assert!(!shown.is_empty());
        authored
            .bindings
            .get_mut("#tag")
            .unwrap()
            .invalidate_projection();
        publish(&mut ui, &[(OWNER, 2, &Arc::new(authored.clone()))]);
        harness.paint(&mut ui, false);
        assert!(
            harness.painted().is_empty(),
            "invalidation hides cached artwork immediately"
        );
        ui.set_hud_projection(Some(camera()), &|_| None);
        harness.paint(&mut ui, false);
        assert!(
            harness.painted().is_empty(),
            "the destination camera cannot restore an old anchor"
        );
        authored.bindings.insert(
            "#tag".into(),
            serde_json::from_value(json!({"project":{"world":[0,0,-10]}})).unwrap(),
        );
        publish(&mut ui, &[(OWNER, 3, &Arc::new(authored))]);
        harness.paint(&mut ui, false);
        assert!(
            harness.painted().is_empty(),
            "a fresh anchor still waits for projection"
        );
        ui.set_hud_projection(Some(camera()), &|_| None);
        harness.paint(&mut ui, false);
        assert_eq!(harness.painted(), shown);
    }
}

/// Panel surfaces also hide projected controls before their first camera pass.
#[test]
fn panel_projections_hide_without_an_available_anchor() {
    let mut authored = (*surface(bindings())).clone();
    let mut document: Value = serde_json::from_str(&authored.document).unwrap();
    document["root"]["controls"] = json!([document["root"]["controls"][3].clone()]);
    authored.document = document.to_string();
    let mut panel = crate::test_support::sample_panel();
    panel.controls.clear();
    panel.surface = authored;
    let mut ui = ModUi::default();
    let mut harness = Harness::new();
    harness.paint(&mut ui, false);
    ui.set_mod_panel(OWNER, Some(&panel)).unwrap();
    ui.set_mod_panel_open(true);
    harness.paint(&mut ui, true);
    assert!(
        harness.painted().is_empty(),
        "initial publication has no camera"
    );
    ui.set_hud_projection(Some(camera()), &|_| None);
    harness.paint(&mut ui, true);
    let shown = harness.painted();
    assert!(!shown.is_empty());
    panel
        .surface
        .bindings
        .get_mut("#tag")
        .unwrap()
        .invalidate_projection();
    ui.set_mod_panel(OWNER, Some(&panel)).unwrap();
    harness.paint(&mut ui, true);
    assert!(
        harness.painted().is_empty(),
        "invalidation hides before projection"
    );
    ui.set_hud_projection(Some(camera()), &|_| None);
    harness.paint(&mut ui, true);
    assert!(harness.painted().is_empty());
    panel.surface.bindings.insert(
        "#tag".into(),
        serde_json::from_value(json!({"project":{"world":[0,0,-10]}})).unwrap(),
    );
    ui.set_mod_panel(OWNER, Some(&panel)).unwrap();
    ui.set_hud_projection(Some(camera()), &|_| None);
    harness.paint(&mut ui, true);
    assert_eq!(harness.painted(), shown);
}

#[test]
fn several_mods_draw_in_load_order_and_keep_their_own_caches() {
    let mut ui = ModUi::default();
    let mut harness = Harness::new();
    let first = surface(bindings());
    let second = surface(json!({"#a":[200,200]}));
    publish(&mut ui, &[(OWNER, 1, &first), (OTHER, 7, &second)]);
    harness.paint(&mut ui, false);
    let both = harness.nodes.len();
    assert_eq!(ui.hud_surface_paints(OTHER), [1, 1, 1, 1]);
    publish(&mut ui, &[(OTHER, 7, &second)]);
    harness.paint(&mut ui, false);
    assert!(harness.nodes.len() < both);
    assert_eq!(
        ui.hud_surface_paints(OTHER),
        [1, 1, 1, 1],
        "the remaining layer keeps its cache"
    );
    assert!(ui.hud_surface_bindings(OWNER).is_none());
    publish(&mut ui, &[]);
    assert!(!ui.has_hud_surfaces());
}

/// A panel whose 80 by 80 node at (100, 100) embeds the `card` control.
fn embedding_panel() -> ui::mod_panel::Panel {
    let mut panel = crate::test_support::sample_panel();
    panel.controls.clear();
    panel.surface = serde_json::from_value(json!({"screen":"menu.root",
        "document":json!({"namespace":"menu","root":{"type":"panel","controls":[
            {"preview":{"type":"custom","renderer":"cinnabar_hud_control","control":"card",
                "size":[80,80],"offset":[100,100],"anchor_from":"top_left","anchor_to":"top_left"}}]}})
        .to_string()}))
    .unwrap();
    panel
}

/// Asserts the embedded card's painted bounds. The 40 by 20 card fits the 80 by 80 node at
/// twice its size, centred vertically, so its 20 by 10 top-left fill paints 80 by 40 logical
/// pixels from (200, 240), within one pixel of antialiasing.
fn assert_embedded_card(harness: &Harness) {
    let painted = harness.painted();
    assert_eq!(painted.len(), 1);
    for (edge, expected) in painted[0].iter().zip([200., 240., 280., 280.]) {
        assert!((edge - expected).abs() <= 1., "{painted:?}");
    }
}

#[test]
fn panels_embed_named_hud_controls_fitted_and_centred() {
    let mut ui = ModUi::default();
    let mut harness = Harness::new();
    publish(&mut ui, &[(OWNER, 1, &surface(bindings()))]);
    harness.paint(&mut ui, false);
    let panel = embedding_panel();
    ui.set_mod_panel(OWNER, Some(&panel)).unwrap();
    ui.set_mod_panel_open(true);
    harness.paint(&mut ui, true);
    assert_embedded_card(&harness);
    ui.set_mod_panel(OTHER, Some(&panel)).unwrap();
    harness.paint(&mut ui, true);
    assert!(
        harness.painted().is_empty(),
        "another owner's panel embeds nothing"
    );
}

/// The open panel reports its root in panel units, the HUD GUI units one panel unit spans and
/// its offset from the HUD root, including while a reference size is fitted to the window.
#[test]
fn panel_layouts_map_panel_units_to_the_hud_root() {
    let mut ui = ModUi::default();
    let mut harness = Harness::new();
    let mut panel = embedding_panel();
    ui.set_mod_panel(OWNER, Some(&panel)).unwrap();
    harness.paint(&mut ui, true);
    assert!(ui.mod_panel_layout().is_none(), "a closed panel has none");
    ui.set_mod_panel_open(true);
    harness.paint(&mut ui, true);
    let layout = |size, scale| ui::mod_hud::PanelLayout {
        size,
        scale,
        offset: [0., 0.],
    };
    assert_eq!(
        ui.mod_panel_layout(),
        Some((OWNER, layout([640., 360.], 1.)))
    );
    panel.reference_size = Some([1280., 720.]);
    ui.set_mod_panel(OWNER, Some(&panel)).unwrap();
    harness.paint(&mut ui, true);
    assert_eq!(
        ui.mod_panel_layout(),
        Some((OWNER, layout([1280., 720.], 0.5))),
        "the reference size fits the 640 by 360 HUD root at half scale"
    );
    ui.set_mod_panel_open(false);
    assert!(ui.mod_panel_layout().is_none());
}

/// A panel opened before any HUD frame lays its embeds out against the root that frame
/// would place.
#[test]
fn panels_embed_hud_controls_before_the_first_hud_frame() {
    let mut ui = ModUi::default();
    let mut harness = Harness::new();
    publish(&mut ui, &[(OWNER, 1, &surface(bindings()))]);
    ui.set_mod_panel(OWNER, Some(&embedding_panel())).unwrap();
    ui.set_mod_panel_open(true);
    harness.paint(&mut ui, true);
    assert_embedded_card(&harness);
}

/// Paint transforms bound to `#cinnabar_scale` resize one cached control, as live resize
/// previews do, and vector paths draw as on panels.
#[test]
fn bound_paint_transforms_and_vector_paths_draw_and_repaint_their_control() {
    let surface = |scale: f64| {
        Arc::new(
            serde_json::from_value::<Surface>(json!({"screen":"hud.root",
                "document":json!({"namespace":"hud","root":{"type":"panel","controls":[
                    {"scaled":{"type":"panel","size":[40,20],"offset":[100,100],
                        "anchor_from":"top_left","anchor_to":"top_left",
                        "cinnabar_transform":{"scale":1},
                        "bindings":[{"binding_name":"#scale",
                            "binding_name_override":"#cinnabar_scale"}],
                        "controls":[{"fill":rect([0.,0.],json!([]))}]}},
                    {"path":{"type":"custom","renderer":"cinnabar_vector_path","size":[20,20],
                        "offset":[10,10],"anchor_from":"top_left","anchor_to":"top_left",
                        "paths":[[[0,0],[1,1]]],"view_box":[0,0,1,1],"stroke_width":2,
                        "closed":false,"color":[1,1,1,1]}}
                ]}}).to_string(),
                "bindings":{"#scale":scale}}))
            .unwrap(),
        )
    };
    let mut ui = ModUi::default();
    let mut harness = Harness::new();
    publish(&mut ui, &[(OWNER, 1, &surface(1.))]);
    harness.paint(&mut ui, false);
    assert!(!harness.painted().is_empty());
    let small = harness.nodes.clone();
    publish(&mut ui, &[(OWNER, 2, &surface(2.))]);
    harness.paint(&mut ui, false);
    assert_eq!(ui.hud_surface_paints(OWNER), [2, 1]);
    assert_ne!(
        harness.nodes, small,
        "the bound scale reaches the painted control"
    );
}

/// Republishing unchanged layers each frame keeps every cache and allocates nothing.
#[test]
fn unchanged_publication_allocates_nothing() {
    let mut ui = ModUi::default();
    let first = surface(bindings());
    let second = surface(json!({"#a":[200,200]}));
    publish(&mut ui, &[(OWNER, 1, &first), (OTHER, 2, &second)]);
    let (_, allocation) =
        test_alloc::measure(|| publish(&mut ui, &[(OWNER, 1, &first), (OTHER, 2, &second)]));
    assert_eq!(allocation.calls, 0);
    publish(&mut ui, &[(OTHER, 2, &second), (OWNER, 1, &first)]);
    assert!(ui.hud_surface_bindings(OWNER).is_some() && ui.hud_surface_bindings(OTHER).is_some());
}

/// [`surface`] with a label reading `#fps` over its own authored `#fps` property, and an eased
/// `#b`.
fn removable(bindings: Value) -> Arc<Surface> {
    let mut surface = (*surface(bindings)).clone();
    let mut document: Value = serde_json::from_str(&surface.document).unwrap();
    document["root"]["controls"].as_array_mut().unwrap().push(
        json!({"fps":{"type":"label","text":"#fps","#fps":"--","offset":[200,10],
            "anchor_from":"top_left","anchor_to":"top_left",
            "bindings":[{"binding_name":"#fps"}]}}),
    );
    surface.document = document.to_string();
    surface.transitions = serde_json::from_value(json!({"#b":{"duration":0.5}})).unwrap();
    Arc::new(surface)
}

/// A control reading a removed binding draws as in a surface published without it: unbound
/// rather than null, and a control that floated on a removed projection, or one that became a
/// plain value, stays in place.
#[test]
fn removed_bindings_draw_as_never_bound() {
    let full = || {
        let mut values = bindings();
        values["#fps"] = json!("60");
        values
    };
    let names = ["#a", "#b", "#helmet", "#tag", "#fps"];
    let mut cases: Vec<(String, Value)> = names
        .iter()
        .map(|name| vec![*name])
        .chain(std::iter::once(names.to_vec()))
        .map(|removed| {
            let mut without = full();
            for name in &removed {
                without.as_object_mut().unwrap().remove(*name);
            }
            (format!("removing {removed:?}"), without)
        })
        .collect();
    let mut placed = full();
    placed["#tag"] = json!([30, 30]);
    cases.push(("placing #tag".into(), placed));
    for (case, next) in cases {
        let draw = |published: &[Arc<Surface>]| {
            let mut ui = ModUi::default();
            let mut harness = Harness::new();
            harness.icon = Some(IconRef {
                page: 0,
                uv: [0, 0, 16, 16],
                glint: false,
                color_mask: None,
            });
            // An empty frame places the HUD root the layers lay out and project against.
            harness.paint(&mut ui, false);
            for (revision, surface) in (1..).zip(published) {
                publish(&mut ui, &[(OWNER, revision, surface)]);
                ui.set_hud_projection(Some(camera()), &|_| None);
                harness.paint(&mut ui, false);
            }
            (
                ui.hud_surface_bindings(OWNER).unwrap(),
                harness.painted(),
                harness.nodes.clone(),
            )
        };
        let patched = draw(&[removable(full()), removable(next.clone())]);
        let fresh = draw(&[removable(next)]);
        assert_eq!(patched.0, fresh.0, "bindings after {case}");
        assert_eq!(patched.1, fresh.1, "painted bounds after {case}");
        assert!(patched.2 == fresh.2, "painted nodes after {case}");
    }
}

/// Overlapping subtrees draw in the same order as the unsplit engine document, including
/// after a definition update adds a descendant layer.
#[test]
fn descendant_layers_keep_global_paint_order() {
    let mut authored = (*surface(json!({}))).clone();
    let mut document: Value = serde_json::from_str(&authored.document).unwrap();
    document["root"]["controls"] = json!([
        {"front":{"type":"panel","controls":[{"text":{"type":"label","text":"HUD",
            "layer":10,"size":[20,10],"anchor_from":"top_left","anchor_to":"top_left"}}]}},
        {"back":rect([0.,0.],json!([]))}
    ]);
    document["root"]["controls"][1]["back"]["layer"] = json!(5);
    authored.document = document.to_string();
    let mut whole = authored.clone();
    document["root"] = json!({"type":"panel","size":["100%","100%"],
        "controls":[{"whole":document["root"].clone()}]});
    whole.document = document.to_string();
    let mut harness = Harness::new();
    let mut ui = ModUi::default();
    publish(&mut ui, &[(OWNER, 1, &Arc::new(whole))]);
    harness.paint(&mut ui, false);
    let expected: Vec<_> = harness
        .nodes
        .iter()
        .filter(|node| *node.visual() != ui::UiVisual::None)
        .map(|node| node.visual().clone())
        .collect();
    assert!(
        expected
            .iter()
            .any(|draw| matches!(draw, ui::UiVisual::Text { .. }))
    );
    for update in [false, true] {
        let mut ui = ModUi::default();
        if update {
            let mut initial = authored.clone();
            let mut document: Value = serde_json::from_str(&initial.document).unwrap();
            document["root"]["controls"][0]["front"]["controls"][0]["text"]["layer"] = json!(0);
            initial.document = document.to_string();
            publish(&mut ui, &[(OWNER, 1, &Arc::new(initial))]);
            harness.paint(&mut ui, false);
        }
        publish(&mut ui, &[(OWNER, 2, &Arc::new(authored.clone()))]);
        harness.paint(&mut ui, false);
        let actual: Vec<_> = harness
            .nodes
            .iter()
            .filter(|node| *node.visual() != ui::UiVisual::None)
            .map(|node| node.visual().clone())
            .collect();
        assert_eq!(actual, expected, "definition update: {update}");
    }
}

/// A binding that stops naming an item stays replaced when another icon refreshes.
#[test]
fn item_type_changes_survive_icon_refreshes() {
    for replacement in [
        json!(""),
        json!({"project":{"world":[0,0,-10]}}),
        json!(0.75),
    ] {
        let mut first = (*surface(bindings())).clone();
        let mut document: Value = serde_json::from_str(&first.document).unwrap();
        let icon = document["root"]["controls"][2].clone();
        let mut other = icon.clone();
        other["icon"]["bindings"][0]["binding_name"] = json!("#other");
        document["root"]["controls"] = json!([{"card":{"type":"panel","controls":[icon,other]}}]);
        first.document = document.to_string();
        first
            .bindings
            .insert("#other".into(), first.bindings["#helmet"].clone());
        let mut ui = ModUi::default();
        let mut harness = Harness::new();
        harness.icon = Some(IconRef {
            page: 0,
            uv: [0, 0, 16, 16],
            glint: false,
            color_mask: None,
        });
        publish(&mut ui, &[(OWNER, 1, &Arc::new(first.clone()))]);
        harness.paint(&mut ui, false);
        first.bindings.insert(
            "#helmet".into(),
            serde_json::from_value(replacement.clone()).unwrap(),
        );
        if replacement.is_number() {
            first.transitions =
                serde_json::from_value(json!({"#helmet":{"duration":1,"from":0}})).unwrap();
        }
        publish(&mut ui, &[(OWNER, 2, &Arc::new(first))]);
        ui.set_hud_projection(Some(camera()), &|_| None);
        harness.paint(&mut ui, false);
        let expected = ui.hud_surface_bindings(OWNER).unwrap()["#helmet"].clone();
        for icon in [None, harness.icon] {
            harness.icon = icon;
            harness.paint(&mut ui, false);
            assert_eq!(
                ui.hud_surface_bindings(OWNER).unwrap()["#helmet"],
                expected,
                "replacement: {replacement}"
            );
        }
    }
}

/// Removing the last transition, or one of several, immediately restores its target.
#[test]
fn removed_transitions_restore_authored_targets() {
    for keep_other in [false, true] {
        let mut first = (*surface(json!({"#a":[10,10],"#b":1}))).clone();
        first.transitions = serde_json::from_value(json!({
            "#a":{"duration":1,"from":[0,0]},"#b":{"duration":1,"from":0}
        }))
        .unwrap();
        let mut layer = layer::Layer::new(
            OWNER,
            1,
            Arc::new(first.clone()),
            ActorAuthority::default(),
            [640., 360.],
        );
        layer.advance(0., true, [640., 360.]);
        layer.advance(0.5, true, [640., 360.]);
        assert_eq!(layer.resolved()["#b"], Scalar::Num(0.5));
        first.transitions.remove("#b");
        if !keep_other {
            first.transitions.clear();
        }
        assert!(layer.publish(2, Arc::new(first), ActorAuthority::default()));
        layer.advance(0.5, true, [640., 360.]);
        assert_eq!(layer.resolved()["#b"], Scalar::Num(1.));
        layer.advance(0.75, true, [640., 360.]);
        assert_eq!(layer.resolved()["#b"], Scalar::Num(1.));
        if keep_other {
            assert_eq!(layer.resolved()["#a"], Scalar::Json(json!([7.5, 7.5])));
        }
    }
}

/// Both surface paths publish the admitted artwork into the renderer's collection input.
#[test]
fn hud_collection_art_matches_panel_renderer_inputs() {
    let art = json!({"paths":[[[0.,0.],[12.,8.],[0.,8.]]],"view_box":[0.,0.,12.,8.],
        "stroke_width":2,"closed":false});
    let surface: Surface = serde_json::from_value(json!({"screen":"hud.root",
        "document":json!({"namespace":"hud","root":{"type":"panel","controls":[
            {"tiles":{"type":"panel","cinnabar_collection":"tiles"}}]}}).to_string(),
        "collections":{"tiles":{"definition":{"columns":1,"viewport":[40,20],
            "template":{"screen":"tile.root","document":json!({"namespace":"tile","root":{
                "type":"custom","renderer":"cinnabar_vector_path","size":[20,20],
                "bindings":[
                    {"binding_name":"#item_art_paths","binding_name_override":"#paths"},
                    {"binding_name":"#item_art_view_box","binding_name_override":"#view_box"},
                    {"binding_name":"#item_art_stroke_width","binding_name_override":"#stroke_width"},
                    {"binding_name":"#item_art_closed","binding_name_override":"#closed"}]
            }}).to_string()},"items":[{"art":art}]},"order":[0]}}
    })).unwrap();
    surface.validate_hud().unwrap();
    let mut panel = crate::test_support::sample_panel();
    panel.surface = surface.clone();
    let mut panel_data = json_ui::DataSource::new();
    crate::mod_panel::surface::collections(&panel, &mut panel_data);
    let layer = layer::Layer::new(
        OWNER,
        1,
        Arc::new(surface),
        ActorAuthority::default(),
        [640., 360.],
    );
    let render = |catalog: &json_ui::Catalog, reference: &str, data: &json_ui::DataSource| {
        let control = json_ui::resolve(catalog, reference, &Context::default())
            .control
            .unwrap();
        let context = Context::default();
        let library = json_ui::CatalogLibrary {
            catalog,
            context: &context,
        };
        let bound = json_ui::bind(&control, data, &library);
        let metrics = ArtMetrics;
        let laid = json_ui::layout(
            &bound,
            [640., 360.],
            &json_ui::LayoutEnv {
                text: &metrics,
                textures: &metrics,
            },
        );
        json_ui::emit(
            &laid,
            &json_ui::LayoutEnv {
                text: &metrics,
                textures: &metrics,
            },
        )
        .into_iter()
        .filter_map(|node| match node.draw {
            json_ui::Draw::Custom { renderer, data }
                if renderer == json_ui::vector_path::RENDERER =>
            {
                Some(data)
            }
            _ => None,
        })
        .collect::<Vec<_>>()
    };
    let catalog = crate::mod_panel::surface::catalog(&panel.surface).unwrap();
    let expected = render(&catalog, &panel.surface.screen, &panel_data);
    assert_eq!(expected.len(), 1);
    assert_eq!(expected[0]["paths"], art["paths"]);
    let actual: Vec<_> = layer
        .parts
        .iter()
        .flat_map(|part| {
            let document = document::Document::build(&panel.surface).unwrap();
            render(&document.catalog, &part.spec.reference, part.data())
        })
        .collect();
    assert!(
        expected
            .iter()
            .all(|data| json_ui::vector_path::parse(data).is_some())
    );
    assert!(
        actual
            .iter()
            .map(json_ui::vector_path::parse)
            .eq(expected.iter().map(json_ui::vector_path::parse)),
        "HUD and panel must supply the same vector artwork"
    );
}

struct ArtMetrics;
impl json_ui::TextMeasure for ArtMetrics {
    /// The art fixture contains no text.
    fn extent(&self, _: &str) -> [f64; 2] {
        [0.; 2]
    }
}
impl json_ui::TextureSource for ArtMetrics {
    /// The art fixture contains no sprites.
    fn texture(&self, _: &str) -> Option<json_ui::TextureMeta> {
        None
    }
}

/// Template-like properties on ordinary controls never become recursive host traversal.
#[test]
fn ordinary_controls_ignore_grid_template_properties() {
    let mut authored = (*surface(json!({}))).clone();
    let mut document: Value = serde_json::from_str(&authored.document).unwrap();
    let mut first = rect([10., 10.], json!([]));
    first["grid_item_template"] = json!("hud.root");
    document["root"]["controls"] =
        json!([{ "first":first }, { "second":rect([40.,10.], json!([])) }]);
    authored.document = document.to_string();
    authored.validate_hud().unwrap();
    let mut ui = ModUi::default();
    let mut harness = Harness::new();
    publish(&mut ui, &[(OWNER, 1, &Arc::new(authored))]);
    harness.paint(&mut ui, false);
    assert!(!harness.painted().is_empty());
}

/// Removing the last item resolver repaints even when its scalar still equals icon index 0.
#[test]
fn replacing_the_last_item_with_its_index_removes_cached_artwork() {
    let mut ui = ModUi::default();
    let mut harness = Harness::new();
    harness.icon = Some(IconRef {
        page: 0,
        uv: [0, 0, 16, 16],
        glint: false,
        color_mask: None,
    });
    publish(&mut ui, &[(OWNER, 1, &surface(bindings()))]);
    harness.paint(&mut ui, false);
    let has_icon = |harness: &Harness| {
        harness
            .nodes
            .iter()
            .any(|node| matches!(node.visual(), ui::UiVisual::Sprite { .. }))
    };
    assert!(has_icon(&harness), "the initial item must paint");
    let mut changed = bindings();
    changed["#helmet"] = json!(0);
    publish(&mut ui, &[(OWNER, 2, &surface(changed))]);
    harness.paint(&mut ui, false);
    assert!(
        !has_icon(&harness),
        "the obsolete item artwork must disappear"
    );
}
