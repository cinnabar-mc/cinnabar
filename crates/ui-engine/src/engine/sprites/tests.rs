use crate::{
    atlas::ServerAtlas,
    engine::{
        FormEngine, ScreenArt,
        screen::{PaintFrame, ScreenRegistry},
    },
    sources::{ImageWork, RemoteImage, RemoteSource},
    test_support::fixture_font,
    text_metrics::TextMetrics,
};
use assets::{RuntimeUiAssets, encode_ui_catalog};
use json_ui::{Catalog, Context, DataSource};
use std::sync::Arc;
use ui::{DpiScale, SafeArea, TextLayoutCache, UiVisual};

struct Offline;
impl RemoteSource for Offline {
    /// Tooltip fixtures never request downloads.
    fn state(&self, _: &str) -> RemoteImage {
        RemoteImage::Failed
    }
}

#[test]
fn translucent_nine_slice_covers_every_pixel_once() {
    static REGISTRY: std::sync::OnceLock<ScreenRegistry> = std::sync::OnceLock::new();
    let registry = REGISTRY.get_or_init(|| ScreenRegistry::new(Context::desktop()));
    let bytes = encode_ui_catalog([1; 32], &[], &[], &[], &[]).unwrap();
    let mut engine = FormEngine::new(
        Arc::new(RuntimeUiAssets::decode(&bytes).unwrap()),
        Catalog::default(),
        0,
        registry,
        Arc::new(Offline),
        ImageWork::inline(),
    );
    let mut png = Vec::new();
    image::RgbaImage::from_pixel(16, 16, image::Rgba([20, 10, 30, 255]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    engine.set_server_atlas(
        ServerAtlas::new(
            &[
                ("textures/fixture.png".into(), png),
                (
                    "textures/fixture.json".into(),
                    br#"{"nineslice_size":4,"base_size":[16,16]}"#.to_vec(),
                ),
            ],
            None,
            1,
        )
        .with_image_work(ImageWork::inline()),
        1,
    );
    let mut catalog = Catalog::default();
    catalog.overlay_text(
        "ui/fixture.json",
        r#"{"namespace":"fixture","root":{"type":"panel","controls":[{"background":{
            "type":"image","texture":"textures/fixture","alpha":0.5,"size":[38.25,18.25],
            "offset":[10.1,4.1],"anchor_from":"top_left","anchor_to":"top_left"}}]}}"#,
    );
    let root = json_ui::resolve(&catalog, "fixture.root", &Context::desktop())
        .control
        .unwrap();
    let root = json_ui::bind(&root, &DataSource::new(), &json_ui::EmptyLibrary);
    let font = fixture_font();
    for dpi in [1.0, 1.25, 2.0] {
        for scale in [2, 3] {
            let mut nodes = Vec::new();
            let mut next = 1;
            engine
                .draw(
                    ScreenArt::default(),
                    &mut PaintFrame {
                        layouts: &mut TextLayoutCache::new(4, 1 << 20),
                        font: &font,
                        metrics: TextMetrics::for_viewport(
                            [1280, 720],
                            DpiScale::new(dpi).unwrap(),
                            Some(scale),
                        ),
                        solid_page: 0,
                        safe_area: SafeArea::ZERO,
                        content: [1280.0 / dpi, 720.0 / dpi],
                        translate: &|_| None,
                        language: [0; 3],
                        now: 0.0,
                        nodes: &mut nodes,
                        next: &mut next,
                        renderers: &mut [],
                    },
                    |env, size| {
                        Some(json_ui::render_bound(
                            root.clone(),
                            size,
                            env,
                            &Default::default(),
                        ))
                    },
                )
                .unwrap();
            let rects: Vec<_> = nodes
                .iter()
                .filter(|n| {
                    matches!(
                        n.visual(),
                        UiVisual::Sprite { .. } | UiVisual::StyledSprite { .. }
                    )
                })
                .map(|n| {
                    let r = n.bounds();
                    [
                        r.min().x() * dpi,
                        r.min().y() * dpi,
                        r.max().x() * dpi,
                        r.max().y() * dpi,
                    ]
                })
                .collect();
            assert_eq!(rects.len(), 9);
            let x0 = rects
                .iter()
                .map(|r| r[0])
                .fold(f32::INFINITY, f32::min)
                .round() as i32;
            let y0 = rects
                .iter()
                .map(|r| r[1])
                .fold(f32::INFINITY, f32::min)
                .round() as i32;
            let x1 = rects
                .iter()
                .map(|r| r[2])
                .fold(f32::NEG_INFINITY, f32::max)
                .round() as i32;
            let y1 = rects
                .iter()
                .map(|r| r[3])
                .fold(f32::NEG_INFINITY, f32::max)
                .round() as i32;
            for y in y0..y1 {
                for x in x0..x1 {
                    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                    let coverage = rects
                        .iter()
                        .filter(|r| px >= r[0] && px < r[2] && py >= r[1] && py < r[3])
                        .count();
                    assert_eq!(
                        coverage, 1,
                        "DPI {dpi}, scale {scale}, pixel {x},{y}: overlapping or missing background slices"
                    );
                }
            }
        }
    }
}
