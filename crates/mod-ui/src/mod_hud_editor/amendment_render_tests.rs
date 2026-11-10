//! The production painter retains catalogs, layouts and text across unchanged editor frames.

use super::*;

struct NoImages;
impl ui_engine::sources::RemoteSource for NoImages {
    /// The original editor fixture has no remote artwork.
    fn state(&self, _: &str) -> ui_engine::sources::RemoteImage {
        ui_engine::sources::RemoteImage::Failed
    }
}

/// Creates a production painter without local game carriers or a graphics device.
fn engine() -> FormEngine {
    use ui_engine::{engine::screen::ScreenRegistry, sources::ImageWork};
    static REGISTRY: std::sync::OnceLock<ScreenRegistry> = std::sync::OnceLock::new();
    let bytes = assets::encode_ui_catalog([1; 32], &[], &[], &[], &[]).unwrap();
    FormEngine::new(
        Arc::new(assets::RuntimeUiAssets::decode(&bytes).unwrap()),
        Catalog::default(),
        0,
        REGISTRY.get_or_init(|| ScreenRegistry::new(Context::desktop())),
        Arc::new(NoImages),
        ImageWork::inline(),
    )
}

/// Paints one editor frame through the same path used by the client.
fn paint(
    ui: &mut HudEditor,
    engine: &FormEngine,
    font: &assets::RuntimeFontCatalog,
    layouts: &mut ui::TextLayoutCache,
    now: f64,
) {
    ui.paint(
        Some(engine),
        &mut PaintFrame {
            layouts,
            font,
            metrics: ui_engine::text_metrics::TextMetrics::for_viewport(
                [1280, 720],
                ui::DpiScale::new(1.).unwrap(),
                Some(2),
            ),
            solid_page: 0,
            safe_area: ui::SafeArea::ZERO,
            content: [1280., 720.],
            translate: &|_| None,
            language: [0; 3],
            now,
            nodes: &mut Vec::new(),
            next: &mut 1,
            renderers: &mut [],
        },
        true,
    );
    assert!(ui.is_open());
}

/// A short distance must not let its wider annotation cover the selected widget or nearby keys.
#[test]
fn measurement_captions_keep_selected_and_neighboring_artwork_clear() {
    let hud: Hud = serde_json::from_value(serde_json::json!({"cards":[
        {"id":"fps","anchor":"top_left","offset":[12,37],"width":56,"row_height":18,"rows":[{"label":"FPS","value":"144"}]},
        {"id":"keys","anchor":"top_left","offset":[0,60],"width":70,"row_height":26,"rows":[{"label":"W","value":""},{"label":"A S D","value":""},{"label":"SPACE","value":""}]}
    ]})).unwrap();
    let mut ui = HudEditor::new(&hud).unwrap();
    let editor = &mut ui;
    editor.viewport = [400., 240.];
    editor.selected = Some(0);
    let data = editor.data();
    for axis in 0..2 {
        assert_eq!(
            data.global_value(&format!("#measurement_{axis}_visible")),
            Some(&Scalar::Bool(true))
        );
        let Scalar::Json(value) = data
            .global_value(&format!("#measurement_{axis}_pill"))
            .unwrap()
        else {
            panic!("caption position")
        };
        let point: [f64; 2] = serde_json::from_value(value.clone()).unwrap();
        for (index, card) in editor.draft.cards.iter().enumerate() {
            let at = editor.card_origin(index);
            let size = cards::dimensions(card);
            assert!(
                point[0] + feedback::pill_size(if axis == 0 { "12" } else { "37" })[0] <= at[0]
                    || point[0] >= at[0] + size[0]
                    || point[1] + MEASUREMENT_PILL_HEIGHT <= at[1]
                    || point[1] >= at[1] + size[1],
                "caption covers {}",
                card.id
            );
        }
    }
}

/// Opens an editor over `cards` and paints its first frame.
fn painted(
    ui: &mut HudEditor,
    engine: &FormEngine,
    font: &assets::RuntimeFontCatalog,
    layouts: &mut ui::TextLayoutCache,
    cards: Vec<ui::mod_hud::Card>,
) {
    *ui = HudEditor::new(&Hud {
        cards,
        ..Default::default()
    })
    .unwrap();
    paint(ui, engine, font, layouts, 1.);
}

/// Distance pills carry a bare number and stay inside the viewport when clamped at either edge.
#[test]
fn distance_pills_show_bare_numbers_and_fit_at_viewport_edges() {
    let viewport = [640., 360.];
    for rect in [
        ([246., 0.], [148., 4.]),
        ([246., 356.], [148., 4.]),
        ([0., 100.], [30., 20.]),
        ([610., 100.], [30., 20.]),
        ([300., 160.], [40., 40.]),
    ] {
        let mut data = DataSource::new();
        feedback::measurements(&mut data, viewport, Some(rect), &[]);
        for axis in 0..2 {
            let global = |suffix: &str| {
                data.global_value(&format!("#measurement_{axis}_{suffix}"))
                    .cloned()
                    .unwrap()
            };
            let Scalar::Bool(visible) = global("visible") else {
                panic!("visibility")
            };
            let Scalar::Text(label) = global("label") else {
                panic!("label")
            };
            if !visible {
                continue;
            }
            assert!(
                !label.is_empty() && label.bytes().all(|b| b.is_ascii_digit()),
                "{label:?} must be a bare number"
            );
            let Scalar::Num(width) = global("pill_width") else {
                panic!("pill width")
            };
            let Scalar::Json(pill) = global("pill") else {
                panic!("pill offset")
            };
            let size = [width, MEASUREMENT_PILL_HEIGHT];
            assert!(width <= 20., "{label} pill is {width} px wide");
            for (n, extent) in size.into_iter().enumerate() {
                let at = pill[n].as_f64().unwrap();
                assert!(
                    at >= 0. && at + extent <= viewport[n],
                    "{rect:?} axis {axis}"
                );
            }
        }
    }
}

/// The painted label is centred inside its pill at the compact font scale.
#[test]
fn distance_pill_label_paints_inside_its_background() {
    let engine = engine();
    let font = ui_engine::test_support::fixture_font();
    let mut layouts = ui::TextLayoutCache::new(64, 1 << 24);
    let mut ui = HudEditor::new(&Hud::default()).unwrap();
    let card = ui::mod_hud::Card {
        id: "fps".into(),
        position: Some([0.5, 0.]),
        ..Default::default()
    };
    painted(&mut ui, &engine, &font, &mut layouts, vec![card]);
    ui.selected = Some(0);
    ui.data_dirty = true;
    paint(&mut ui, &engine, &font, &mut layouts, 2.);
    let editor = &ui;
    let Some(Scalar::Text(label)) = editor.data.global_value("#measurement_0_label") else {
        panic!("label")
    };
    let measurement = EditorStyle::default()
        .measurement
        .map(|v| (v * 255.).round() as u8);
    let nodes = editor.screen.nodes();
    // Node rectangles are in paint units; the default card surface is 148 GUI px wide.
    let px = nodes
        .iter()
        .find(|node| node.key.ends_with("card_0/surface"))
        .expect("card surface")
        .dest
        .w
        / f64::from(ui::mod_hud::DEFAULT_CARD_WIDTH);
    let background = nodes
        .iter()
        .find(|node| node.key.contains("measurement_0_pill") && matches!(&node.draw, json_ui::Draw::Custom { data, .. }
            if data.get("#color").or(data.get("color")).and_then(json_ui::color_value) == Some(measurement)))
        .expect("pill background");
    let (text, scale) = nodes
        .iter()
        .find_map(|node| match &node.draw {
            json_ui::Draw::Text { text, scale, .. } if text == label => Some((node, *scale)),
            _ => None,
        })
        .expect("pill label");
    let [bx, by, bw, bh] = [
        background.dest.x,
        background.dest.y,
        background.dest.w,
        background.dest.h,
    ];
    assert!(
        (bh / px - MEASUREMENT_PILL_HEIGHT).abs() < 0.01,
        "pill height {}",
        bh / px
    );
    assert!(
        (bw / px - feedback::pill_size(label)[0]).abs() < 0.01,
        "pill width {}",
        bw / px
    );
    assert!(bx >= 0. && by >= 0.);
    let center = [
        text.dest.x + text.dest.w * 0.5,
        text.dest.y + text.dest.h * 0.5,
    ];
    assert!((center[0] - (bx + bw * 0.5)).abs() < 0.5);
    assert!((center[1] - (by + bh * 0.5)).abs() < 0.5);
    assert!(text.dest.h <= bh);
    assert!(
        (scale - MEASUREMENT_FONT_SCALE as f32).abs() < 0.01,
        "font scale {scale}"
    );
}

/// Outlines appear only on hover or selection, through data bindings rather than a rebuild.
#[test]
fn card_outlines_follow_hover_and_selection_without_rebuilding() {
    let engine = engine();
    let font = ui_engine::test_support::fixture_font();
    let mut layouts = ui::TextLayoutCache::new(64, 1 << 24);
    let mut ui = HudEditor::new(&Hud::default()).unwrap();
    let cards = vec![
        ui::mod_hud::Card {
            id: "a".into(),
            position: Some([0.1, 0.1]),
            background_radius: 8.,
            ..Default::default()
        },
        ui::mod_hud::Card {
            id: "b".into(),
            position: Some([0.9, 0.9]),
            ..Default::default()
        },
    ];
    painted(&mut ui, &engine, &font, &mut layouts, cards);
    let style = EditorStyle::default();
    let color = |ui: &HudEditor, index: usize| match ui
        .data
        .global_value(&format!("#bounds_{index}_color"))
    {
        Some(Scalar::Json(value)) => serde_json::from_value::<[f32; 4]>(value.clone()).unwrap(),
        other => panic!("bounds colour {other:?}"),
    };
    assert_eq!(color(&ui, 0), [0.; 4]);
    assert_eq!(color(&ui, 1), [0.; 4]);
    let editor = &ui;
    assert!(
        editor.screen.nodes().iter().any(|node| matches!(&node.draw,
            json_ui::Draw::Custom { data, .. } if data.get("radius").and_then(serde_json::Value::as_f64) == Some(8.))),
        "preview background keeps its authored radius"
    );
    let catalog = Arc::clone(editor.catalog.as_ref().unwrap());
    let frame = editor.frame.as_ref().unwrap();
    let hit = frame
        .hits
        .iter()
        .find(|hit| hit.pressed.as_deref() == Some("hud.card:0"))
        .unwrap();
    let at = [
        (hit.rect.x + hit.rect.w * 0.5) as f32 * frame.scale + frame.origin[0],
        (hit.rect.y + hit.rect.h * 0.5) as f32 * frame.scale + frame.origin[1],
    ];
    ui.pointer(at, false, false, &mut []);
    paint(&mut ui, &engine, &font, &mut layouts, 2.);
    assert_eq!(color(&ui, 0), style.outline);
    assert_eq!(color(&ui, 1), [0.; 4]);
    let editor = &mut ui;
    assert!(Arc::ptr_eq(&catalog, editor.catalog.as_ref().unwrap()));
    editor.selected = Some(1);
    editor.data_dirty = true;
    paint(&mut ui, &engine, &font, &mut layouts, 3.);
    assert_eq!(color(&ui, 1), style.selected);
    assert!(Arc::ptr_eq(&catalog, ui.catalog.as_ref().unwrap()));
}

/// Fixed touch previews keep measured bounds, cached chrome and one-shot save results.
#[test]
fn fixed_touch_preview_retains_geometry_and_chrome_without_a_personal_panel() {
    let engine = engine();
    let font = ui_engine::test_support::fixture_font();
    let mut layouts = ui::TextLayoutCache::new(64, 1 << 24);
    let mut editor = HudEditor::new(&Hud {
        cards: vec![ui::mod_hud::Card {
            id: "jump".into(),
            editor_label: "Jump".into(),
            editor_size: Some([76., 52.]),
            anchor: ui::mod_hud::Anchor::BottomRight,
            offset: [12., 20.],
            width: 1.,
            cells: vec![ui::mod_hud::Cell::default()],
            ..Default::default()
        }],
        ..Default::default()
    })
    .unwrap();
    paint(&mut editor, &engine, &font, &mut layouts, 1.);
    let bounds = editor.rectangles().next().unwrap().1;
    assert_eq!([bounds[2], bounds[3]], [76., 52.]);
    for action in [
        "hud.save",
        "hud.cancel",
        "hud.undo",
        "hud.redo",
        "hud.reset",
        "hud.grid",
        "hud.snap",
    ] {
        assert!(editor.action_bounds(action).is_some(), "missing {action}");
    }
    let catalog = Arc::clone(editor.catalog.as_ref().unwrap());
    let passes = editor.screen.passes;
    let shaped = layouts.built_layout_count();
    paint(&mut editor, &engine, &font, &mut layouts, 2.);
    assert!(Arc::ptr_eq(&catalog, editor.catalog.as_ref().unwrap()));
    assert_eq!(editor.screen.passes, passes);
    assert_eq!(layouts.built_layout_count(), shaped);
    assert_eq!(editor.rectangles().next().unwrap().1, bounds);
    editor.host_key("Enter", Default::default());
    let result = editor.take_result().unwrap();
    assert!(result.saved);
    result.validate().unwrap();
    assert_eq!(
        result.placements[0].anchor,
        ui::mod_hud::Anchor::BottomRight
    );
    assert_eq!(result.placements[0].offset, [12., 20.]);
    assert_eq!(result.placements[0].position, None);
    assert!(editor.take_result().is_none());
}
