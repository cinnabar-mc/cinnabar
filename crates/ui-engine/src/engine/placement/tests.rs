use std::{
    collections::HashMap,
    sync::{Arc, LazyLock},
};

use json_ui::{Catalog, Context, ViewState, render_bound};
use serde_json::{Value, json};
use ui::{DpiScale, IconRef, SafeArea, TextLayoutCache, UiNode, UiVisual};

use super::*;
use crate::{
    engine::{
        FormEngine, ScreenArt,
        screen::{PaintFrame, ScreenRegistry},
    },
    sources::{ImageWork, RemoteImage, RemoteSource},
    test_support::fixture_font,
    text_metrics::TextMetrics,
};

static REGISTRY: LazyLock<ScreenRegistry> =
    LazyLock::new(|| ScreenRegistry::new(Context::desktop()));

struct NoImages;

impl RemoteSource for NoImages {
    /// All artwork in these fixtures is supplied directly.
    fn state(&self, _: &str) -> RemoteImage {
        RemoteImage::Failed
    }
}

/// An original clipped sidebar with one image, beneath a full-screen holder.
fn document() -> Value {
    json!({
        "namespace": "fixture",
        "sidebar": {
            "type": "panel", "size": [40, 20], "offset": [10, 20],
            "anchor_from": "top_left", "anchor_to": "top_left", "clips_children": true,
            "controls": [{"background": {
                "type": "image", "texture": "textures/fixture", "size": ["100%", "100%"]
            }}]
        },
        "root": {"type": "panel", "size": ["100%", "100%"], "controls": [
            {"holder": {"type": "panel", "size": ["100%", "100%"], "controls": [
                {"sidebar@fixture.sidebar": {}}
            ]}}
        ]}
    })
}

/// Paints an original document through the engine using synthetic font and image fixtures.
fn paint(
    document: &Value,
    placement: Option<ControlPlacement<'_>>,
    dpi: f32,
    scale: u8,
) -> Vec<UiNode> {
    let catalog = Catalog::from_files([
        ("ui/_global_variables.json", b"{}".as_slice()),
        (
            "ui/_ui_defs.json",
            br#"{"ui_defs":["ui/fixture.json"]}"#.as_slice(),
        ),
        ("ui/fixture.json", document.to_string().as_bytes()),
    ])
    .unwrap();
    let bytes = assets::encode_ui_catalog([1; 32], &[], &[], &[], &[]).unwrap();
    let engine = FormEngine::new(
        Arc::new(assets::RuntimeUiAssets::decode(&bytes).unwrap()),
        catalog,
        0,
        &REGISTRY,
        Arc::new(NoImages),
        ImageWork::inline(),
    );
    let images = HashMap::from([(
        "textures/fixture".into(),
        IconRef {
            page: 1,
            uv: [0, 0, 40, 20],
            color_mask: None,
            glint: false,
        },
    )]);
    let font = fixture_font();
    let mut layouts = TextLayoutCache::new(4, 1 << 20);
    let mut nodes = Vec::new();
    let mut next = 1;
    let bound = json_ui::resolve(engine.catalog(), "fixture.root", &Context::desktop())
        .control
        .unwrap();
    engine
        .draw(
            ScreenArt {
                placement,
                images: Some(&images),
                ..Default::default()
            },
            &mut PaintFrame {
                layouts: &mut layouts,
                font: &font,
                metrics: TextMetrics::for_viewport(
                    [1200, 900],
                    DpiScale::new(dpi).unwrap(),
                    Some(scale),
                ),
                solid_page: 0,
                safe_area: SafeArea::ZERO,
                content: [1200. / dpi, 900. / dpi],
                translate: &|_| None,
                language: [0; 3],
                now: 0.,
                nodes: &mut nodes,
                next: &mut next,
                renderers: &mut [],
            },
            |env, root| Some(render_bound(bound, root, env, &ViewState::default())),
        )
        .unwrap()
        .expect("fixture paints");
    nodes
}

/// Returns each primitive's artwork, absolute rectangle and clip in paint order.
fn primitives(nodes: &[UiNode]) -> Vec<(&UiVisual, [f32; 4], [f32; 4])> {
    nodes
        .iter()
        .filter(|node| !matches!(node.visual(), UiVisual::None))
        .map(|node| {
            let clip = nodes
                .iter()
                .find(|parent| Some(parent.id()) == node.parent())
                .unwrap()
                .bounds();
            let bounds = node.bounds();
            (
                node.visual(),
                [
                    clip.min().x() + bounds.min().x(),
                    clip.min().y() + bounds.min().y(),
                    clip.min().x() + bounds.max().x(),
                    clip.min().y() + bounds.max().y(),
                ],
                [
                    clip.min().x(),
                    clip.min().y(),
                    clip.max().x(),
                    clip.max().y(),
                ],
            )
        })
        .collect()
}

#[test]
fn placement_preserves_clip_and_size_animation_geometry() {
    let placement = ControlPlacement {
        definition: ["fixture", "sidebar"],
        anchor: [0.; 2],
        offset: [80., 60.],
    };
    for kind in ["clip", "size"] {
        let mut doc = document();
        let end = if kind == "clip" {
            json!(0.5)
        } else {
            json!([20, 10])
        };
        doc["motion"] = json!({"anim_type": kind, "from": end, "to": end, "duration": 1});
        let target = if kind == "clip" {
            &mut doc["sidebar"]["controls"][0]["background"]
        } else {
            &mut doc["sidebar"]
        };
        target["anims"] = json!(["@fixture.motion"]);
        target["clip_direction"] = json!("left");
        let baseline = paint(&doc, None, 1., 1);
        let moved = paint(&doc, Some(placement), 1., 1);
        let baseline = primitives(&baseline);
        let moved = primitives(&moved);
        assert_eq!(baseline.len(), 1, "{kind} baseline");
        assert_eq!(moved.len(), 1, "{kind} remains visible");
        assert_eq!(moved[0].0, baseline[0].0, "{kind} keeps its sampled UV");
        assert_eq!(
            baseline[0].1[2] - baseline[0].1[0],
            20.,
            "{kind} animates width"
        );
        for edge in 0..4 {
            let shift = [70., 40.][edge % 2];
            assert_eq!(
                moved[0].1[edge] - baseline[0].1[edge],
                shift,
                "{kind} destination"
            );
            assert_eq!(moved[0].2[edge] - baseline[0].2[edge], shift, "{kind} clip");
        }
    }
}

#[test]
fn placement_moves_only_the_resolved_instance_and_uses_only_its_bounds() {
    let mut doc = document();
    let other = json!({
        "type": "image", "texture": "textures/fixture", "size": [5, 5],
        "offset": [2, 3], "anchor_from": "top_left", "anchor_to": "top_left"
    });
    doc["root"]["controls"][0]["holder"]["controls"] = json!([
        {"sidebar": {"collection_index": 2, "controls": [{"decoy": other}], "type": "panel", "size": ["100%", "100%"]}},
        {"sidebar@fixture.sidebar": {"collection_index": 2}},
        {"sidebar@fixture.sidebar": {"collection_index": 2}}
    ]);
    doc["root"]["controls"].as_array_mut().unwrap().push(json!({
        "other_panel": {"type": "panel", "size": ["100%", "100%"], "controls": [{"sidebar": other}, {"sidebar_decoration": other}]}
    }));
    let baseline = paint(&doc, None, 1., 1);
    let moved = paint(
        &doc,
        Some(ControlPlacement {
            definition: ["fixture", "sidebar"],
            anchor: [1.; 2],
            offset: [0.; 2],
        }),
        1.,
        1,
    );
    let baseline = primitives(&baseline);
    let moved = primitives(&moved);
    assert_eq!(baseline.len(), 5);
    assert_eq!(moved.len(), baseline.len());
    for index in 0..baseline.len() {
        if index == 1 {
            assert_eq!(moved[index].1, [1160., 880., 1200., 900.]);
        } else {
            assert_eq!(moved[index], baseline[index], "unrelated primitive {index}");
        }
    }
}

#[test]
fn placement_preserves_image_and_label_alignment_at_fractional_display_dpi() {
    let mut doc = document();
    doc["sidebar"]["offset"] = json!([10.1, 20.1]);
    doc["sidebar"]["controls"]
        .as_array_mut()
        .unwrap()
        .push(json!({"label": {
            "type": "label", "text": "0", "size": [20, 10], "offset": [0.2, 0.2],
            "anchor_from": "top_left", "anchor_to": "top_left"
        }}));
    let placement = ControlPlacement {
        definition: ["fixture", "sidebar"],
        anchor: [0.; 2],
        offset: [80.4, 60.4],
    };
    let baseline = paint(&doc, None, 1.5, 3);
    let moved = paint(&doc, Some(placement), 1.5, 3);
    let baseline = primitives(&baseline);
    let moved = primitives(&moved);
    assert_eq!(baseline.len(), 2);
    assert_eq!(moved.len(), 2);
    assert!(matches!(moved[1].0, UiVisual::Text { .. }));
    for index in 0..2 {
        for axis in 0..2 {
            let physical = (moved[index].1[axis] - baseline[index].1[axis]) * 1.5;
            assert!(
                (physical - [211., 121.][axis]).abs() < 1e-4,
                "primitive {index}, axis {axis}: {physical}"
            );
        }
    }
}

#[test]
fn placement_translation_stays_outside_the_sampled_paint_transform() {
    let transform = json_ui::transform::Affine {
        linear: [0., 2., -2., 0.],
        translation: [5., 7.],
    };
    let mut drawn = Animated {
        dest: json_ui::RectOut {
            x: 10.,
            y: 20.,
            w: 40.,
            h: 20.,
        },
        clip: json_ui::RectOut {
            x: 0.,
            y: 0.,
            w: 100.,
            h: 100.,
        },
        opacity: 1.,
        color: None,
        uv: None,
        repeat: None,
        hidden: false,
        transform: Some(transform),
    };
    let original = transform.apply([drawn.dest.x, drawn.dest.y]);
    translate(&mut drawn, [70., 40.]);
    let moved = drawn.transform.unwrap().apply([drawn.dest.x, drawn.dest.y]);
    assert_eq!(moved, [original[0] + 70., original[1] + 40.]);
}
