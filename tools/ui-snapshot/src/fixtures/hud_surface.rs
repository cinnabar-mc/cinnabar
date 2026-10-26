//! An original mod HUD surface over gameplay, and a panel embedding its named controls.

use super::Fixture;
use json_ui::vanilla::Binding;
use serde_json::{Value, json};
use std::sync::Arc;
use ui::mod_panel::{
    CollectionDefinition, CollectionItem, Panel, Surface, SurfaceCollection, SurfaceValue,
};

/// A sans label at `offset` with literal or bound `text`.
fn label(text: &str, offset: [f64; 2], scale: f64) -> Value {
    let color = Binding::Color.as_str().trim_start_matches('#');
    let mut node = json!({"type":"label","text":text,"offset":offset,"localize":false,
        "anchor_from":"top_left","anchor_to":"top_left","font_type":ui::mod_panel::SANS_FONT_NAME,
        "font_scale_factor":scale,(color):[0.92,0.95,1,1]});
    if text.starts_with('#') {
        node["bindings"] = json!([{"binding_name":text}]);
    }
    node
}

/// A rounded background of `size`.
fn background(size: [f64; 2], alpha: f64) -> Value {
    let color = Binding::Color.as_str().trim_start_matches('#');
    json!({"type":"custom","renderer":"cinnabar_rounded_rectangle","size":size,"radius":4,
        "anchor_from":"top_left","anchor_to":"top_left",(color):[0.03,0.04,0.06,alpha]})
}

/// Three status cards, a scaled keystrokes card and a projected waypoint label. The cards are
/// named controls outside the root that the root instantiates and panels embed; `zoom` is a
/// preview-only caption whose two short lines leave most of its laid-out box empty.
pub(super) fn surface() -> Surface {
    let color = Binding::Color.as_str().trim_start_matches('#');
    let document = json!({"namespace":"status",
        "fps":{"type":"panel","size":[86,20],"controls":[
            {"bg":background([86.,20.],0.75)},{"value":label("#fps",[8.,6.],0.8)}]},
        "armor":{"type":"panel","size":[110,48],"controls":[
            {"bg":background([110.,48.],0.75)},
            {"title":label("Armor",[8.,6.],0.7)},
            {"line_1":label("#armor_line_1",[8.,20.],0.8)},
            {"line_2":label("#armor_line_2",[8.,32.],0.8)}]},
        "keys":{"type":"panel","size":[66,43],"cinnabar_transform":{"scale":1},
            "bindings":[{"binding_name":"#keys_scale","binding_name_override":"#cinnabar_scale"}],
            "controls":[
                {"w":{"type":"custom","renderer":"cinnabar_rounded_rectangle","size":[20,20],
                    "offset":[23,0],"radius":3,"anchor_from":"top_left","anchor_to":"top_left",
                    (color):[1,0.35,0.23,1]}},
                {"a":{"type":"custom","renderer":"cinnabar_rounded_rectangle","size":[20,20],
                    "offset":[0,23],"radius":3,"anchor_from":"top_left","anchor_to":"top_left",
                    (color):[0.2,0.23,0.28,1]}},
                {"s":{"type":"custom","renderer":"cinnabar_rounded_rectangle","size":[20,20],
                    "offset":[23,23],"radius":3,"anchor_from":"top_left","anchor_to":"top_left",
                    (color):[0.2,0.23,0.28,1]}},
                {"d":{"type":"custom","renderer":"cinnabar_rounded_rectangle","size":[20,20],
                    "offset":[46,23],"radius":3,"anchor_from":"top_left","anchor_to":"top_left",
                    (color):[0.2,0.23,0.28,1]}}]},
        "zoom":{"type":"panel","size":[110,48],"controls":[
            {"level":label("4.0x zoom",[30.,12.],0.6)},
            {"hint":label("Hold to zoom",[26.,26.],0.6)}]},
        "root":{"type":"panel","size":["100%","100%"],"controls":[
            {"g_fps@status.fps":{"anchor_from":"top_left","anchor_to":"top_left","offset":[8,8]}},
            {"g_armor@status.armor":{"anchor_from":"top_right","anchor_to":"top_right",
                "offset":[-8,8]}},
            {"g_keys@status.keys":{"anchor_from":"bottom_left","anchor_to":"bottom_left",
                "offset":[26,-40]}},
            {"home":{"type":"panel","size":[0,0],"anchor_from":"top_left","anchor_to":"top_left",
                "bindings":[{"binding_name":"#home","binding_name_override":"#offset"},
                    {"binding_name":"#home_visible","binding_name_override":"#visible"}],
                "controls":[{"plate":{"type":"panel","size":[64,16],"offset":[-32,-8],
                    "anchor_from":"top_left","anchor_to":"top_left","controls":[
                        {"bg":background([64.,16.],0.5)},{"text":label("Home 12m",[6.,4.],0.7)}]}}]}}
        ]}
    })
    .to_string();
    serde_json::from_value(
        json!({"screen":"status.root","document":document,"bindings":{
        "#fps":"240 FPS","#armor_line_1":"Helmet 363/363","#armor_line_2":"Boots 412/429",
        "#keys_scale":1.5,"#home":{"project":{"world":[0,0,-12]}}}}),
    )
    .expect("valid original HUD surface")
}

/// The gameplay layer under a camera looking down negative Z, so the waypoint shows.
pub(super) fn gameplay() -> Fixture {
    Fixture::HudSurface(Box::new(surface()))
}

/// A module browser whose tiles embed the HUD surface's cards by name, each fitted by its
/// painted ink so the small zoom caption fills its tile as the cards do, beside a large preview
/// fitted by its laid-out box.
pub(super) fn panel() -> Fixture {
    let color = Binding::Color.as_str().trim_start_matches('#');
    let tile = Surface {
        screen: "status_tile.root".into(),
        document: json!({"namespace":"status_tile","root":{"type":"panel","size":[140,90],
            "controls":[
                {"bg":{"type":"custom","renderer":"cinnabar_rounded_rectangle","size":[132,82],
                    "radius":8,(color):[0.11,0.13,0.17,1]}},
                {"name":{"type":"label","text":"#item_name","size":[116,18],"offset":[0,-25],
                    "bindings":[{"binding_name":"#item_name"}],"localize":false,
                    "font_type":ui::mod_panel::SANS_FONT_NAME,"font_scale_factor":0.75,
                    (color):[0.9,0.93,1,1]}},
                {"preview":{"type":"custom","renderer":"cinnabar_hud_control",
                    "control_binding":"#item_card","size":[120,54],"offset":[0,8],
                    "fit":0.78,"max_scale":2.4}}]}})
        .to_string(),
        bindings: Default::default(),
        transitions: Default::default(),
        collections: Default::default(),
    };
    let items = [
        ("fps", "FPS"),
        ("armor", "Armor"),
        ("keys", "Keystrokes"),
        ("zoom", "Zoom"),
    ]
    .map(|(id, name)| CollectionItem {
        values: [
            ("#item_card".into(), SurfaceValue::Text(id.into())),
            ("#item_name".into(), SurfaceValue::Text(name.into())),
        ]
        .into(),
        ..Default::default()
    });
    let document = json!({"namespace":"status_menu","screen":{"type":"screen","size":[640,360],
    "controls":[
        {"background":{"type":"custom","renderer":"cinnabar_rounded_rectangle",
            "size":[620,340],"radius":12,(color):[0.06,0.07,0.09,1]}},
        {"title":label("Mods",[34.,25.],1.6)},
        {"grid":{"type":"panel","cinnabar_collection":"grid","size":[300,190],
            "offset":[34,118],"anchor_from":"top_left","anchor_to":"top_left"}},
        {"stage":{"type":"custom","renderer":"cinnabar_rounded_rectangle","size":[230,190],
            "offset":[170,24],"radius":10,(color):[0.11,0.13,0.17,1]}},
        {"large_preview":{"type":"custom","renderer":"cinnabar_hud_control","control":"armor",
            "size":[210,120],"offset":[170,24]}}
    ]}})
    .to_string();
    let surface = Surface {
        screen: "status_menu.screen".into(),
        document,
        bindings: Default::default(),
        transitions: Default::default(),
        collections: [(
            "grid".into(),
            SurfaceCollection {
                reveal: None,
                definition: Arc::new(CollectionDefinition {
                    template: tile,
                    columns: 2,
                    viewport: [300., 190.],
                    items: items.into(),
                }),
                order: (0..4).collect(),
            },
        )]
        .into(),
    };
    let panel = Panel {
        surface,
        reference_size: Some([640., 360.]),
        toggle_key: "ShiftRight".into(),
        controls: vec![],
        widget_colors: None,
        capture_key: false,
        show_gameplay_hud: false,
        motion: false,
    };
    Fixture::ModPanel(Box::new(panel), Some(Box::new(self::surface())), None)
}

/// A right-handed perspective camera at the origin looking down negative Z, as the client's
/// camera projects, with `aspect` width over height.
pub(crate) fn camera(aspect: f32) -> [f32; 16] {
    let (near, far, fov) = (0.1_f32, 1000.0_f32, 1.0_f32);
    let height = 1. / (fov * 0.5).tan();
    let range = far / (near - far);
    [
        height / aspect,
        0.,
        0.,
        0.,
        0.,
        height,
        0.,
        0.,
        0.,
        0.,
        range,
        -1.,
        0.,
        0.,
        range * near,
        0.,
    ]
}
