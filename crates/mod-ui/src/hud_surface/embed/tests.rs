//! Embedded HUD controls fitted by layout, by a fixed scale and by painted ink, through the
//! production engine.

use std::sync::Arc;

use serde_json::{Value, json};
use ui::mod_panel::{
    Surface, SurfaceValue,
    hud_control::{MAX_SCALE, MIN_SCALE},
};

use super::{FitCache, FitKey};
use crate::hud_surface::{ActorAuthority, PublishedHudSurface, tests::Harness};
use crate::{ModUi, PanelUiOwner};

const OWNER: PanelUiOwner = PanelUiOwner::Environment(0);
/// The embedding node: 80 by 80 panel units at (100, 100), which the harness paints as 160 by
/// 160 logical pixels from (200, 200).
const NODE: [f32; 4] = [200., 200., 360., 360.];

/// A HUD document of two caption controls whose laid-out box is far larger than their ink:
/// `caption`, a 20 by 10 rectangle at (40, 15) in a 100 by 40 box, and `zoom`, a short label.
fn hud() -> Arc<Surface> {
    Arc::new(
        serde_json::from_value(json!({"screen":"hud.root",
            "document":json!({"namespace":"hud",
                "root":{"type":"panel","size":["100%","100%"]},
                "caption":{"type":"panel","size":[100,40],"controls":[{"fill":{
                    "type":"custom","renderer":"cinnabar_rounded_rectangle","size":[20,10],
                    "offset":[40,15],"anchor_from":"top_left","anchor_to":"top_left",
                    "color":[1,0,0,1]}}]},
                "zoom":{"type":"panel","size":[100,40],"controls":[{"text":{
                    "type":"label","text":"Hold to zoom","localize":false,
                    "font_scale_factor":0.5}}]}
            }).to_string()}))
        .unwrap(),
    )
}

/// A panel embedding `control` in the 80 by 80 node with `fitting` properties, and `bindings`.
fn panel(control: &str, fitting: Value, bindings: Value) -> ui::mod_panel::Panel {
    let mut node = json!({"type":"custom","renderer":"cinnabar_hud_control","control":control,
        "size":[80,80],"offset":[100,100],"anchor_from":"top_left","anchor_to":"top_left"});
    node.as_object_mut()
        .unwrap()
        .extend(fitting.as_object().unwrap().clone());
    let mut panel = crate::test_support::sample_panel();
    panel.controls.clear();
    panel.surface = serde_json::from_value(json!({"screen":"menu.root",
        "document":json!({"namespace":"menu","root":{"type":"panel",
            "controls":[{"preview":node}]}}).to_string(),
        "bindings":bindings}))
    .unwrap();
    panel
}

/// The [`hud`] layer and `panel`, open and painted once.
fn opened(panel: &ui::mod_panel::Panel) -> (ModUi, Harness) {
    let mut ui = ModUi::default();
    let mut harness = Harness::new();
    let hud = hud();
    ui.set_hud_surfaces(std::iter::once(PublishedHudSurface {
        owner: OWNER,
        revision: 1,
        surface: &hud,
        authority: ActorAuthority::default(),
    }));
    ui.set_mod_panel(OWNER, Some(panel)).unwrap();
    ui.set_mod_panel_open(true);
    harness.paint(&mut ui, true);
    (ui, harness)
}

/// The [`hud`] layer and an open panel embedding `control` with `fitting`, painted once.
fn embedded(control: &str, fitting: Value) -> (ModUi, Harness) {
    opened(&panel(control, fitting, json!({})))
}

/// The embedded control's painted ink at its HUD size, in logical pixels.
fn ink(ui: &ModUi, control: &str) -> [f32; 2] {
    let bounds = ui.hud_surfaces.layers[0].embedded[control]
        .part
        .as_ref()
        .unwrap()
        .bounds
        .unwrap();
    [bounds.width(), bounds.height()]
}

/// The single painted rectangle, `[left, top, right, bottom]`.
fn painted(harness: &Harness) -> [f32; 4] {
    let painted = harness.painted();
    assert_eq!(painted.len(), 1, "{painted:?}");
    painted[0]
}

/// Asserts the painted ink fills `fraction` of the node along its limiting axis, centred.
fn assert_ink_fit(harness: &Harness, fraction: f32) {
    let [left, top, right, bottom] = painted(harness);
    let node = [NODE[2] - NODE[0], NODE[3] - NODE[1]];
    let size = [right - left, bottom - top];
    assert!(
        ((size[0] / node[0]).max(size[1] / node[1]) - fraction).abs() < 0.001,
        "{size:?} fills {fraction} of {node:?}"
    );
    for (axis, min) in [left, top].into_iter().enumerate() {
        let expected = NODE[axis] + (node[axis] - size[axis]) * 0.5;
        assert!(
            (min - expected).abs() < 0.001,
            "{min} centred at {expected}"
        );
    }
}

/// Asserts each painted edge within one pixel of antialiasing.
fn assert_painted(harness: &Harness, expected: [f32; 4]) {
    let painted = painted(harness);
    for (edge, expected) in painted.iter().zip(expected) {
        assert!(
            (edge - expected).abs() <= 1.,
            "{painted:?}, expected {expected}"
        );
    }
}

#[test]
fn ink_fit_magnifies_a_caption_that_layout_fit_draws_small() {
    // The 100 by 40 box fits the node at 0.8 times its HUD size, so the 20 by 10 fill draws
    // 32 by 16 pixels from (264, 272).
    let (_, layout) = embedded("caption", json!({}));
    assert_painted(&layout, [264., 272., 296., 288.]);
    let (_, fitted) = embedded("caption", json!({"fit":0.78}));
    assert_ink_fit(&fitted, 0.78);
    let (_, full) = embedded("caption", json!({"fit":1}));
    assert_ink_fit(&full, 1.);

    let (_, label) = embedded("zoom", json!({}));
    let (_, magnified) = embedded("zoom", json!({"fit":0.78}));
    assert_ink_fit(&magnified, 0.78);
    let width = |rect: [f32; 4]| rect[2] - rect[0];
    assert!(
        width(painted(&magnified)) > 2. * width(painted(&label)),
        "{:?} against {:?}",
        painted(&magnified),
        painted(&label)
    );
}

#[test]
fn ink_fit_caps_at_max_scale_and_ignores_scale() {
    // Uncapped, the fill would draw at 3.12 times its HUD size.
    let (ui, capped) = embedded("caption", json!({"fit":0.78,"max_scale":2.4}));
    let ink = ink(&ui, "caption");
    let [left, top, right, bottom] = painted(&capped);
    assert!(((right - left) - ink[0] * 2.4).abs() < 0.001);
    assert!(((bottom - top) - ink[1] * 2.4).abs() < 0.001);
    assert!((left - (NODE[0] + (160. - ink[0] * 2.4) * 0.5)).abs() < 0.001);

    let (_, scaled) = embedded("caption", json!({"fit":0.78,"max_scale":2.4,"scale":0.5}));
    assert_eq!(painted(&scaled), painted(&capped), "fit ignores scale");

    let mut large = panel("caption", json!({"fit":1}), json!({}));
    let mut document: Value = serde_json::from_str(&large.surface.document).unwrap();
    document["root"]["controls"][0]["preview"]["size"] = json!([300, 300]);
    large.surface.document = document.to_string();
    let (_, harness) = opened(&large);
    let [left, _, right, _] = painted(&harness);
    assert!(
        ((right - left) - ink[0] * MAX_SCALE).abs() < 0.001,
        "max_scale defaults to {MAX_SCALE}"
    );
}

#[test]
fn scale_without_fit_draws_the_laid_out_box_at_that_multiple_and_centred() {
    // The 100 by 40 box at half its HUD size spans 100 by 40 pixels from (230, 260).
    let (_, scaled) = embedded("caption", json!({"scale":0.5}));
    assert_painted(&scaled, [270., 275., 290., 285.]);
    let (_, uncapped) = embedded("caption", json!({"scale":0.5,"max_scale":MIN_SCALE}));
    assert_eq!(
        painted(&uncapped),
        painted(&scaled),
        "max_scale applies only to fit"
    );
}

#[test]
fn bound_fitting_values_update_the_fit_and_out_of_bounds_values_draw_nothing() {
    let fitting = json!({"fit":0.78,"max_scale":2.4,"scale":1,"bindings":[
        {"binding_name":"#fraction","binding_name_override":"#fit"},
        {"binding_name":"#cap","binding_name_override":"#max_scale"},
        {"binding_name":"#size","binding_name_override":"#scale"}]});
    let (mut ui, mut harness) = opened(&panel("caption", fitting.clone(), json!({})));
    for (fraction, cap, size, visible) in [
        (0.5, 5., 1., true),
        (1., 5., 1., true),
        (0.78, 5., MAX_SCALE as f64, true),
        (0., 5., 1., false),
        (1.1, 5., 1., false),
        (0.78, 0.1, 1., false),
        (0.78, 6., 1., false),
        (0.78, 5., 0.1, false),
    ] {
        let bindings = json!({"#fraction":fraction,"#cap":cap,"#size":size});
        let panel = panel("caption", fitting.clone(), bindings);
        panel.surface.validate().unwrap();
        ui.set_mod_panel(OWNER, Some(&panel)).unwrap();
        harness.paint(&mut ui, true);
        let case = format!("fit {fraction}, max_scale {cap}, scale {size}");
        if visible {
            assert_ink_fit(&harness, fraction as f32);
        } else {
            assert!(harness.painted().is_empty(), "{case} draws nothing");
        }
    }
    let mut panel = panel(
        "caption",
        fitting,
        json!({"#fraction":1,"#cap":2,"#size":1}),
    );
    ui.set_mod_panel(OWNER, Some(&panel)).unwrap();
    harness.paint(&mut ui, true);
    let capped = painted(&harness);
    assert!(((capped[2] - capped[0]) - ink(&ui, "caption")[0] * 2.).abs() < 0.001);
    panel
        .surface
        .bindings
        .insert("#cap".into(), SurfaceValue::Number(3.));
    ui.set_mod_panel(OWNER, Some(&panel)).unwrap();
    harness.paint(&mut ui, true);
    let raised = painted(&harness);
    assert!(((raised[2] - raised[0]) - ink(&ui, "caption")[0] * 3.).abs() < 0.001);
}

#[test]
fn steady_frames_reuse_the_ink_fit_and_changed_inputs_recompute_it() {
    let (mut ui, mut harness) = embedded("caption", json!({"fit":0.78}));
    assert_eq!(ui.mod_panel_embed_fits(), 1);
    let first = painted(&harness);
    for _ in 0..3 {
        harness.paint(&mut ui, true);
    }
    assert_eq!(ui.mod_panel_embed_fits(), 1, "steady frames reuse the fit");
    assert_eq!(painted(&harness), first);
    ui.set_mod_panel(
        OWNER,
        Some(&panel("caption", json!({"fit":0.5}), json!({}))),
    )
    .unwrap();
    harness.paint(&mut ui, true);
    harness.paint(&mut ui, true);
    assert_eq!(ui.mod_panel_embed_fits(), 2, "a new fraction fits once");
    assert_ink_fit(&harness, 0.5);
    let (mut layout, mut harness) = embedded("caption", json!({"scale":1}));
    harness.paint(&mut layout, true);
    assert_eq!(
        layout.mod_panel_embed_fits(),
        0,
        "only ink fits use the cache"
    );
}

#[test]
fn fit_cache_tracks_only_dimensions_and_fitting_parameters() {
    let key = FitKey {
        ink: [400., 100.],
        node: [200., 100.],
        ui_scale: 2.,
        fraction: 0.8,
        max_scale: 2.4,
    };
    let mut cache = FitCache::default();
    assert_eq!(cache.scale(key), 0.2);
    assert!(cache.scale(key) < MIN_SCALE, "ink fits are not floored");
    assert_eq!(cache.computations, 1);
    let (_, allocation) = test_alloc::measure(|| assert_eq!(cache.scale(key), 0.2));
    assert_eq!(allocation.calls, 0);
    assert_eq!(cache.computations, 1);
    for changed in [
        FitKey {
            ink: [200., 100.],
            ..key
        },
        FitKey {
            ink: [400., 200.],
            ..key
        },
        FitKey {
            node: [100., 100.],
            ..key
        },
        FitKey {
            node: [200., 20.],
            ..key
        },
        FitKey {
            ui_scale: 1.,
            ..key
        },
        FitKey {
            fraction: 0.5,
            ..key
        },
        FitKey {
            max_scale: MIN_SCALE,
            ..key
        },
    ] {
        cache.scale(key);
        let before = cache.computations;
        cache.scale(changed);
        assert_eq!(cache.computations, before + 1);
        cache.scale(changed);
        assert_eq!(cache.computations, before + 1);
    }
    assert_eq!(
        cache.scale(FitKey {
            ink: [1., 1.],
            ..key
        }),
        key.max_scale
    );
}
