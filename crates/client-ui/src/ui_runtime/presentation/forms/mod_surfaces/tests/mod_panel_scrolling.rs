use mod_ui::PanelUiOwner;
use serde_json::{Value, json};
use ui::mod_panel::{Control, Panel, Surface, SurfaceValue};

use super::super::super::snapshot;
use super::mod_panel::{frame, point};
use crate::test_support::mini_engine_presentation;
use crate::ui_runtime::presentation::UiPresentationRuntime;
use mod_ui::test_support::sample_panel as panel;

/// Builds a generic extension surface with clipped controls and a bound content height.
fn scroll_panel() -> Panel {
    scroll_panel_with(json!({}), json!({}), json!({}))
}

/// [`scroll_panel`] with extra properties on its scroll view, scrollbar box and track.
fn scroll_panel_with(view: Value, box_extra: Value, track_extra: Value) -> Panel {
    let merge = |mut base: Value, extra: Value| {
        for (key, value) in extra.as_object().into_iter().flatten() {
            base[key] = value.clone();
        }
        base
    };
    let mut panel = panel();
    panel.controls = ["first", "last"]
        .map(|id| Control::Button {
            id: id.into(),
            label: id.into(),
        })
        .into();
    let button = |index, y| json!({"type":"button","size":[90,20],"offset":[0,y],"anchor_from":"top_left","anchor_to":"top_left","button_mappings":[{"from_button_id":"button.menu_select","to_button_id":format!("mod.control:{index}"),"mapping_type":"pressed"}],"controls":[{"fill":{"type":"custom","renderer":"cinnabar_rounded_rectangle","size":["100%","100%"],"color":if index == 0 { [0.2,0.6,0.8,1.] } else { [0.2,0.8,0.4,1.] },"radius":2}},{"label":{"type":"label","size":[90,20],"text":if index == 0 { "First control" } else { "Last control" },"color":[1,1,1,1],"text_alignment":"center","anchor_from":"top_left","anchor_to":"top_left"}}]});
    let content = json!({"type":"panel","size":[90,200],"anchor_from":"top_left","anchor_to":"top_left",
        "bindings":[{"binding_name":"#content_height","binding_name_override":"#size_binding_y_absolute"}],
        "controls":[{"first":button(0,0)},{"last":button(1,180)}]});
    let viewport = json!({"type":"panel","size":[90,50],"anchor_from":"top_left","anchor_to":"top_left",
        "clips_children":true,"controls":[{"content":content}]});
    let track = merge(
        json!({"type":"scroll_track","size":[10,50],"anchor_from":"top_left","anchor_to":"top_left",
        "button_mappings":[{"from_button_id":"button.menu_select","to_button_id":"mod.scroll_track","mapping_type":"pressed"}]}),
        track_extra,
    );
    let thumb = merge(
        json!({"type":"scrollbar_box","size":[10,15],"draggable":"vertical","anchor_from":"top_left","anchor_to":"top_left"}),
        box_extra,
    );
    let bar = json!({"type":"panel","size":[10,50],"offset":[90,0],"anchor_from":"top_left","anchor_to":"top_left",
        "controls":[{"track":track},{"box":thumb}]});
    let list = merge(
        json!({"type":"scroll_view","size":[100,50],"offset":[20,20],"anchor_from":"top_left","anchor_to":"top_left",
        "scroll_view_port":"viewport","scroll_content":"content","scrollbar_track":"track","scrollbar_box":"box",
        "scroll_box_and_track_panel":"bar","scrollbar_track_button":"mod.scroll_track","scroll_speed":25,"always_handle_pointer":true,
        "controls":[{"viewport":viewport},{"bar":bar}]}),
        view,
    );
    panel.surface = Surface { collections: Default::default(),        transitions: Default::default(),
        screen: "extension.screen".into(),
        document: json!({"namespace":"extension","screen":{"type":"screen","size":["100%","100%"],"controls":[{"list":list}]}}).to_string(),
        bindings: std::collections::BTreeMap::from([("#content_height".into(), SurfaceValue::Number(200.))]),
    };
    panel
}

/// Converts a panel's virtual coordinates to its actual window-logical input coordinates.
fn at(presentation: &UiPresentationRuntime, xy: [f64; 2]) -> [f32; 2] {
    let frame = presentation
        .mod_ui()
        .mod_panel_state()
        .unwrap()
        .frame
        .unwrap();
    [
        frame.origin[0] + xy[0] as f32 * frame.scale,
        frame.origin[1] + xy[1] as f32 * frame.scale,
    ]
}

/// Reads the visible first scroll view's layout result after a rendered frame.
fn offset(presentation: &UiPresentationRuntime) -> f64 {
    presentation
        .mod_ui()
        .mod_panel_state()
        .unwrap()
        .frame
        .unwrap()
        .report
        .scrolls
        .values()
        .next()
        .unwrap()
        .offset
}

#[test]
fn wheel_reaches_clipped_controls_and_content_resize_clamps_the_offset() {
    let mut presentation = mini_engine_presentation();
    let mut panel = scroll_panel();
    presentation
        .mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), Some(&panel))
        .unwrap();
    presentation.mod_ui_mut().set_mod_panel_open(true);
    let before = frame(&mut presentation, [1280, 720]);
    snapshot::write(&before, "personal-panel-scroll-before");
    let inside = at(&presentation, [65., 45.]);
    let outside = at(&presentation, [5., 5.]);
    assert!(
        !presentation
            .mod_ui_mut()
            .scroll_mod_panel(outside, -20., false)
    );
    assert!(
        presentation
            .mod_ui_mut()
            .scroll_mod_panel(inside, -20., false)
    );
    let after = frame(&mut presentation, [1280, 720]);
    snapshot::write(&after, "personal-panel-scroll-after");
    assert_eq!(offset(&presentation), 150.);
    let last = point(&presentation, "mod.control:1", 0.5);
    assert_eq!(
        presentation.mod_ui_mut().mod_panel_events(last, true, true)[0].id,
        "last"
    );
    panel
        .surface
        .bindings
        .insert("#content_height".into(), SurfaceValue::Number(50.));
    presentation
        .mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), Some(&panel))
        .unwrap();
    frame(&mut presentation, [1280, 720]);
    assert_eq!(offset(&presentation), 0.);
    panel
        .surface
        .bindings
        .insert("#content_height".into(), SurfaceValue::Number(200.));
    presentation
        .mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), Some(&panel))
        .unwrap();
    frame(&mut presentation, [1280, 720]);
    assert_eq!(offset(&presentation), 0.);
    presentation.mod_ui_mut().set_mod_panel_open(false);
    assert!(
        !presentation
            .mod_ui_mut()
            .scroll_mod_panel(inside, -1., false)
    );
}

#[test]
fn scrollbar_drag_applies_release_position_and_focus_loss_cancels_capture() {
    let mut presentation = mini_engine_presentation();
    presentation
        .mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), Some(&scroll_panel()))
        .unwrap();
    presentation.mod_ui_mut().set_mod_panel_open(true);
    frame(&mut presentation, [1280, 720]);
    let thumb = at(&presentation, [115., 25.]);
    assert!(
        presentation
            .mod_ui_mut()
            .mod_panel_events(thumb, true, true)
            .is_empty()
    );
    let bottom = at(&presentation, [115., 70.]);
    assert!(
        presentation
            .mod_ui_mut()
            .mod_panel_events(bottom, false, false)
            .is_empty()
    );
    frame(&mut presentation, [1280, 720]);
    assert_eq!(offset(&presentation), 150.);
    let inside = at(&presentation, [65., 45.]);
    assert!(
        presentation
            .mod_ui_mut()
            .scroll_mod_panel(inside, 20., false)
    );
    frame(&mut presentation, [1280, 720]);
    presentation
        .mod_ui_mut()
        .mod_panel_events(thumb, true, true);
    presentation.mod_ui_mut().cancel_mod_panel_pointer_input();
    presentation
        .mod_ui_mut()
        .mod_panel_events(bottom, false, true);
    frame(&mut presentation, [1280, 720]);
    assert_eq!(offset(&presentation), 0.);
}

#[test]
fn pixel_wheels_and_track_presses_use_the_drawn_panel_geometry() {
    let mut presentation = mini_engine_presentation();
    presentation
        .mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), Some(&scroll_panel()))
        .unwrap();
    presentation.mod_ui_mut().set_mod_panel_open(true);
    frame(&mut presentation, [1280, 720]);
    let inside = at(&presentation, [65., 45.]);
    let scale = presentation
        .mod_ui()
        .mod_panel_state()
        .unwrap()
        .frame
        .unwrap()
        .scale;
    assert!(
        presentation
            .mod_ui_mut()
            .scroll_mod_panel(inside, -10. * f64::from(scale), true)
    );
    assert!(
        presentation
            .mod_ui_mut()
            .scroll_mod_panel(inside, -10. * f64::from(scale), true)
    );
    assert!(
        !presentation
            .mod_ui_mut()
            .scroll_mod_panel(inside, f64::NAN, true)
    );
    frame(&mut presentation, [1280, 720]);
    assert_eq!(offset(&presentation), 20.);
    let track = at(&presentation, [115., 69.]);
    assert!(
        presentation
            .mod_ui_mut()
            .mod_panel_events(track, true, true)
            .is_empty()
    );
    frame(&mut presentation, [1280, 720]);
    assert_eq!(offset(&presentation), 150.);
}

/// Wheel easing belongs to the mod; pixel input and a mod's opt-out remain immediate.
#[test]
fn mod_wheel_eases_with_screen_animations_off() {
    use crate::ui_runtime::UiRuntime;
    use launcher::menu::settings_options::{SettingId, SettingsOptions};
    use ui::DpiScale;
    let mut target = 0.;
    for motion in [false, true] {
        let mut presentation = mini_engine_presentation();
        let mut options = SettingsOptions::default();
        options.set(SettingId::ScreenAnimations.index(), 0);
        presentation.set_chat_settings_snapshot((std::sync::Arc::new(options), None));
        let mut content = scroll_panel();
        content.motion = motion;
        presentation
            .mod_ui_mut()
            .set_mod_panel(PanelUiOwner::Environment(0), Some(&content))
            .unwrap();
        presentation.mod_ui_mut().set_mod_panel_open(true);
        let build = |p: &mut UiPresentationRuntime, millis| {
            p.build(
                &player_state::PlayerState::new(1),
                &UiRuntime::new(1),
                millis,
                [1280, 720],
                DpiScale::new(1.).unwrap(),
            )
            .unwrap()
        };
        build(&mut presentation, 1_000);
        let inside = at(&presentation, [65., 45.]);
        assert!(
            presentation
                .mod_ui_mut()
                .scroll_mod_panel(inside, -2., false)
        );
        build(&mut presentation, 1_000);
        if motion {
            assert_eq!(offset(&presentation), 0.);
        } else {
            target = offset(&presentation);
            assert!(target > 0., "the immediate gesture must move the content");
        }
        build(&mut presentation, 1_070);
        let middle = offset(&presentation);
        if motion {
            assert!(middle > 0. && middle < target);
        } else {
            assert_eq!(middle, target);
        }
        build(&mut presentation, 1_200);
        assert_eq!(offset(&presentation), target);
        assert!(!presentation.mod_ui().mod_panel_motion_active());
        let scale = presentation
            .mod_ui()
            .mod_panel_state()
            .unwrap()
            .frame
            .unwrap()
            .scale;
        assert!(
            presentation
                .mod_ui_mut()
                .scroll_mod_panel(inside, -10. * f64::from(scale), true)
        );
        build(&mut presentation, 1_200);
        assert_eq!(
            offset(&presentation),
            target + 10.,
            "pixel scrolling stays immediate"
        );
    }
}

/// Opaque state colours of the styled scrollbar: resting, hovered and pressed.
const IDLE: [u8; 3] = [102, 102, 102];
const LIT: [u8; 3] = [230, 230, 230];
const HELD: [u8; 3] = [51, 153, 255];
const TRACK_LIT: [u8; 3] = [0, 204, 102];

/// A full-size fill in `rgb`, one state child of a scrollbar box or track.
fn fill(rgb: [u8; 3]) -> Value {
    json!({"type":"custom","renderer":"cinnabar_rounded_rectangle","size":["100%","100%"],
        "anchor_from":"top_left","anchor_to":"top_left",
        "color":[f64::from(rgb[0]) / 255., f64::from(rgb[1]) / 255., f64::from(rgb[2]) / 255., 1.],"radius":0})
}

/// A scroll panel whose box and track restyle on hover and press, with `view` properties.
fn styled_panel(view: Value) -> Panel {
    scroll_panel_with(
        view,
        json!({"cinnabar_default_control":"idle","cinnabar_hover_control":"lit",
            "cinnabar_pressed_control":"held",
            "controls":[{"idle":fill(IDLE)},{"lit":fill(LIT)},{"held":fill(HELD)}]}),
        json!({"cinnabar_hover_control":"lit","controls":[{"lit":fill(TRACK_LIT)}]}),
    )
}

/// The rasterized colour at a panel's virtual point.
fn colour(
    presentation: &UiPresentationRuntime,
    input: &render_model::UiRenderInput,
    xy: [f64; 2],
) -> [u8; 3] {
    let [x, y] = at(presentation, xy);
    let pixel = snapshot::rasterize(input).get_pixel(x as u32, y as u32).0;
    [pixel[0], pixel[1], pixel[2]]
}

/// The drawn thumb's centre in panel coordinates, or `None` while it is hidden.
fn thumb_centre(presentation: &UiPresentationRuntime) -> Option<[f64; 2]> {
    let state = presentation.mod_ui().mod_panel_state().unwrap();
    let thumb = state.frame.unwrap().report.scrolls.values().next()?.thumb?;
    Some([thumb[0] + thumb[2] / 2., thumb[1] + thumb[3] / 2.])
}

/// Asserts two colours match within rasterization rounding.
fn assert_colour(actual: [u8; 3], expected: [u8; 3], what: &str) {
    assert!(
        actual.iter().zip(expected).all(|(a, e)| a.abs_diff(e) <= 3),
        "{what}: {actual:?} is not {expected:?}"
    );
}

#[test]
fn scrollbar_states_follow_hover_and_drag_with_a_declared_minimum() {
    let mut presentation = mini_engine_presentation();
    let panel = styled_panel(json!({"cinnabar_min_thumb_fraction":0.4}));
    presentation
        .mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), Some(&panel))
        .unwrap();
    presentation.mod_ui_mut().set_mod_panel_open(true);
    let idle = frame(&mut presentation, [1280, 720]);
    snapshot::write(&idle, "mod-scrollbar-idle");
    let thumb = presentation
        .mod_ui()
        .mod_panel_state()
        .unwrap()
        .frame
        .unwrap()
        .report
        .scrolls
        .values()
        .next()
        .unwrap()
        .thumb
        .unwrap();
    assert_eq!(thumb[3], 20., "four tenths of the 50 px track");
    let centre = thumb_centre(&presentation).unwrap();
    assert_colour(colour(&presentation, &idle, centre), IDLE, "resting box");
    let below = [centre[0], 60.];
    assert_ne!(
        colour(&presentation, &idle, below),
        TRACK_LIT,
        "resting track"
    );

    let pointer = at(&presentation, centre);
    presentation
        .mod_ui_mut()
        .mod_panel_events(pointer, false, false);
    let hovered = frame(&mut presentation, [1280, 720]);
    snapshot::write(&hovered, "mod-scrollbar-hover");
    assert_colour(colour(&presentation, &hovered, centre), LIT, "hovered box");

    presentation
        .mod_ui_mut()
        .mod_panel_events(pointer, true, true);
    let lower = at(&presentation, [centre[0], centre[1] + 15.]);
    presentation
        .mod_ui_mut()
        .mod_panel_events(lower, false, true);
    let held = frame(&mut presentation, [1280, 720]);
    snapshot::write(&held, "mod-scrollbar-pressed");
    assert!(offset(&presentation) > 0., "the drag scrolls");
    let moved = thumb_centre(&presentation).unwrap();
    assert_colour(colour(&presentation, &held, moved), HELD, "dragged box");

    presentation
        .mod_ui_mut()
        .mod_panel_events(lower, false, false);
    let released = frame(&mut presentation, [1280, 720]);
    assert_colour(
        colour(&presentation, &released, moved),
        LIT,
        "the released box stays hovered under the pointer",
    );

    let above = [centre[0], 25.];
    let track = at(&presentation, above);
    presentation
        .mod_ui_mut()
        .mod_panel_events(track, false, false);
    let over_track = frame(&mut presentation, [1280, 720]);
    snapshot::write(&over_track, "mod-scrollbar-track-hover");
    assert_colour(
        colour(&presentation, &over_track, above),
        TRACK_LIT,
        "hovered track",
    );
    assert_colour(
        colour(&presentation, &over_track, moved),
        IDLE,
        "box at rest",
    );

    presentation
        .mod_ui_mut()
        .mod_panel_events(track, true, true);
    let pressed_track = frame(&mut presentation, [1280, 720]);
    let jumped = thumb_centre(&presentation).unwrap();
    assert_colour(
        colour(&presentation, &pressed_track, jumped),
        IDLE,
        "a track press leaves the box at rest",
    );
}

/// A builder that renders the presentation at `millis` on the caller's clock.
fn build_at(presentation: &mut UiPresentationRuntime, millis: u64) -> render_model::UiRenderInput {
    presentation
        .build(
            &player_state::PlayerState::new(1),
            &crate::ui_runtime::UiRuntime::new(1),
            millis,
            [1280, 720],
            ui::DpiScale::new(1.).unwrap(),
        )
        .unwrap()
}

#[test]
fn a_scroll_view_sets_its_wheel_easing_under_panel_motion() {
    let mut targets = Vec::new();
    for seconds in [0., 0.5] {
        let mut presentation = mini_engine_presentation();
        let mut content = scroll_panel_with(
            json!({"cinnabar_scroll_seconds":seconds}),
            json!({}),
            json!({}),
        );
        content.motion = true;
        presentation
            .mod_ui_mut()
            .set_mod_panel(PanelUiOwner::Environment(0), Some(&content))
            .unwrap();
        presentation.mod_ui_mut().set_mod_panel_open(true);
        build_at(&mut presentation, 1_000);
        let inside = at(&presentation, [65., 45.]);
        assert!(
            presentation
                .mod_ui_mut()
                .scroll_mod_panel(inside, -2., false)
        );
        build_at(&mut presentation, 1_000);
        let first = offset(&presentation);
        build_at(&mut presentation, 1_300);
        let middle = offset(&presentation);
        build_at(&mut presentation, 1_600);
        let end = offset(&presentation);
        assert!(end > 0.);
        if seconds == 0. {
            assert_eq!(first, end, "zero seconds scrolls at once");
        } else {
            assert_eq!(first, 0.);
            assert!(middle > 0. && middle < end, "still easing after 0.3 s");
        }
        targets.push(end);
    }
    assert_eq!(targets[0], targets[1]);
}

#[test]
fn an_auto_hidden_scrollbar_shows_while_scrolled_and_fades_out() {
    let mut presentation = mini_engine_presentation();
    let panel = styled_panel(json!({"cinnabar_scrollbar_auto_hide":true}));
    presentation
        .mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), Some(&panel))
        .unwrap();
    presentation.mod_ui_mut().set_mod_panel_open(true);
    build_at(&mut presentation, 1_000);
    assert_eq!(thumb_centre(&presentation), None, "hidden at rest");
    assert!(!presentation.mod_ui().mod_panel_motion_active());

    let inside = at(&presentation, [65., 45.]);
    assert!(
        presentation
            .mod_ui_mut()
            .scroll_mod_panel(inside, -2., false)
    );
    let shown = build_at(&mut presentation, 1_000);
    snapshot::write(&shown, "mod-scrollbar-auto-hide-shown");
    let centre = thumb_centre(&presentation).expect("the wheel reveals the box");
    assert_colour(colour(&presentation, &shown, centre), IDLE, "revealed box");
    assert!(presentation.mod_ui().mod_panel_motion_active());
    build_at(&mut presentation, 1_500);
    assert!(thumb_centre(&presentation).is_some(), "still fading");
    let gone = build_at(&mut presentation, 2_200);
    build_at(&mut presentation, 2_200);
    snapshot::write(&gone, "mod-scrollbar-auto-hide-idle");
    assert_eq!(thumb_centre(&presentation), None, "faded out");
    assert!(!presentation.mod_ui().mod_panel_motion_active());

    // The pointer on the track shows the box again for as long as it stays there.
    let track = at(&presentation, [115., 60.]);
    presentation
        .mod_ui_mut()
        .mod_panel_events(track, false, false);
    build_at(&mut presentation, 3_000);
    build_at(&mut presentation, 9_000);
    assert!(
        thumb_centre(&presentation).is_some(),
        "held under the pointer"
    );
    let away = at(&presentation, [5., 5.]);
    presentation
        .mod_ui_mut()
        .mod_panel_events(away, false, false);
    build_at(&mut presentation, 9_000);
    build_at(&mut presentation, 9_500);
    assert!(thumb_centre(&presentation).is_some());
    build_at(&mut presentation, 10_600);
    build_at(&mut presentation, 10_600);
    assert_eq!(thumb_centre(&presentation), None);
}
