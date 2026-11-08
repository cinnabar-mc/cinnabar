//! Empty-catalog click routing through the installed screen and production press handler.

use json_ui::{Context, DataSource, Draw, DrawNode, RectOut, TextAlign, ViewState};
use ui_engine::engine::{FormEngine, ScreenArt, screen::PaintFrame};
use ui_screens::containers::presses::{HeldStackPress, engine_held_stack_press};

use crate::carriers::Error;

/// Paints the result of clicking an empty catalog viewport with an original held-stack icon.
pub(super) fn paint(engine: &FormEngine, frame: &mut PaintFrame<'_>) -> Result<(), Error> {
    let mut catalog = engine.catalog().as_ref().clone();
    catalog.overlay_text(
        "ui/creative_delete.json",
        r#"{
      "namespace": "creative_delete",
      "screen": {"type": "panel", "size": [200, 170], "controls": [
        {"title": {"type": "label", "text": "Creative inventory", "localize": false,
          "size": [180, 15], "offset": [0, -75]}},
        {"catalog@crafting.recipe_book_scroll_panel": {"size": [180, 140], "offset": [0, 5]}}
      ]}
    }"#,
    );
    let context = Context::desktop();
    let root = json_ui::resolve(&catalog, "creative_delete.screen", &context)
        .control
        .ok_or_else(|| Error::Invalid("creative catalog did not resolve".into()))?;
    let mut data = DataSource::new();
    data.set_strict(true);
    let root = json_ui::bind(
        &root,
        &data,
        &json_ui::CatalogLibrary {
            catalog: &catalog,
            context: &context,
        },
    );
    let draw = |art: ScreenArt<'_>, frame: &mut PaintFrame<'_>| {
        engine
            .draw(art, frame, |env, size| {
                Some(json_ui::render_bound(
                    root.clone(),
                    size,
                    env,
                    &ViewState::default(),
                ))
            })
            .map_err(|error| error.to_string())?
            .ok_or_else(|| Error::Invalid("creative catalog did not paint".into()))
    };
    let ready = draw(ScreenArt::default(), frame)?;
    let viewport = ready
        .report
        .scrolls
        .values()
        .find_map(|scroll| scroll.viewport_rect)
        .ok_or_else(|| Error::Invalid("creative catalog viewport is absent".into()))?;
    let point = [
        (viewport[0] + viewport[2] / 2.0) as f32,
        (viewport[1] + viewport[3] / 2.0) as f32,
    ];
    let mut overlay = Vec::new();
    if engine_held_stack_press(&ready, point) != Some(HeldStackPress::Destroy) {
        let clip = RectOut {
            x: 0.0,
            y: 0.0,
            w: 10000.0,
            h: 10000.0,
        };
        let dest = RectOut {
            x: f64::from(point[0]) - 8.0,
            y: f64::from(point[1]) - 8.0,
            w: 16.0,
            h: 16.0,
        };
        for draw in [
            Draw::Solid {
                color: [139, 91, 52, 255],
            },
            Draw::Text {
                text: "64".into(),
                color: [255; 4],
                shadow: true,
                align: TextAlign::Right,
                localize: false,
                options: Default::default(),
                scale: 1.0,
            },
        ] {
            overlay.push(DrawNode {
                name: "held_stack".into(),
                key: String::new(),
                dest,
                clip,
                layer: i32::MAX,
                alpha: 1.0,
                anim: None,
                paint_region: None,
                draw,
                gates: Vec::new(),
            });
        }
    }
    frame.nodes.clear();
    *frame.next = 1;
    draw(
        ScreenArt {
            overlay: &overlay,
            ..Default::default()
        },
        frame,
    )?;
    Ok(())
}
