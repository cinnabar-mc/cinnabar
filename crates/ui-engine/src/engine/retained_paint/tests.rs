//! A retained paint is reused only while everything it read is unchanged and settled.

use std::{
    collections::HashMap,
    sync::{Arc, LazyLock},
};

use json_ui::{Catalog, Context, DataSource};
use serde_json::json;
use ui::{DpiScale, FONT_DESIGN_PIXEL_TEXELS, SafeArea, TextLayoutCache, UiNode};

use super::RetainedPaint;
use crate::{
    atlas::ServerAtlas,
    cached_screen::CachedScreen,
    engine::{
        Canvas, FormEngine, ScreenArt,
        screen::{CustomDraw, CustomRenderer, PaintError, PaintFrame, ScreenRegistry},
    },
    sources::{ImageWork, RemoteImage, RemoteSource},
    test_support::fixture_font,
    text_metrics::TextMetrics,
};

static REGISTRY: LazyLock<ScreenRegistry> =
    LazyLock::new(|| ScreenRegistry::new(Context::desktop()));

struct NoImages;
impl RemoteSource for NoImages {
    /// This fixture never requests network artwork.
    fn state(&self, _: &str) -> RemoteImage {
        RemoteImage::Failed
    }
}

/// A caller renderer that draws nothing but reports the textures it may draw.
#[derive(Default)]
struct Reporter(Vec<&'static str>);

impl CustomRenderer for Reporter {
    /// Leaves every node to the engine's own renderers.
    fn paint(&mut self, _: &CustomDraw<'_>, _: &mut Canvas<'_>) -> Result<bool, PaintError> {
        Ok(false)
    }

    fn textures<'s>(&'s self, _: &dyn Fn(&str) -> bool, texture: &mut dyn FnMut(&'s str)) {
        for path in &self.0 {
            texture(path);
        }
    }
}

/// One screen whose rounded rectangle fades in over its first second.
struct Fixture {
    reporter: Reporter,
    engine: FormEngine,
    catalog: Arc<Catalog>,
    data: Arc<DataSource>,
    screen: CachedScreen,
    retained: RetainedPaint,
    font: Arc<assets::RuntimeFontCatalog>,
    layouts: TextLayoutCache,
    solid_page: u16,
}

impl Fixture {
    fn new() -> Self {
        let bytes = assets::encode_ui_catalog([1; 32], &[], &[], &[], &[]).unwrap();
        let engine = FormEngine::new(
            Arc::new(assets::RuntimeUiAssets::decode(&bytes).unwrap()),
            Catalog::default(),
            0,
            &REGISTRY,
            Arc::new(NoImages),
            ImageWork::inline(),
        );
        let doc = json!({"namespace":"fixture",
            "fade":{"anim_type":"alpha","from":0,"to":1,"duration":1},
            "root":{"type":"panel","size":[100,80],"clips_children":true,"controls":[{"fader":{
                "type":"panel","size":[100,80],"propagate_alpha":true,"anims":["@fixture.fade"],
                "controls":[{"rect":{
                    "type":"custom","renderer":json_ui::rounded::RENDERER,"size":[40,20],
                    "offset":[20,20],"anchor_from":"top_left","anchor_to":"top_left",
                    "color":[0.8,0.6,0.4,1],"radius":6}}]}}]}})
        .to_string();
        let catalog = Catalog::from_files([
            ("ui/_global_variables.json", b"{}".as_slice()),
            (
                "ui/_ui_defs.json",
                br#"{"ui_defs":["ui/fixture.json"]}"#.as_slice(),
            ),
            ("ui/fixture.json", doc.as_bytes()),
        ])
        .unwrap();
        Self {
            reporter: Reporter::default(),
            engine,
            catalog: Arc::new(catalog),
            data: Arc::new(DataSource::new()),
            screen: CachedScreen::default(),
            retained: RetainedPaint::default(),
            font: fixture_font(),
            layouts: TextLayoutCache::new(4, 1 << 20),
            solid_page: 0,
        }
    }

    /// Draws and ends one frame at `now`, through the retained paint when `retained`.
    fn frame(&mut self, now: f64, reuse: bool, retained: bool) -> (Vec<UiNode>, u32) {
        let metrics = TextMetrics::for_viewport([400, 320], DpiScale::new(1.0).unwrap(), Some(1));
        let px = metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        let mut nodes = Vec::new();
        let mut next = 1;
        let Self {
            reporter,
            engine,
            catalog,
            data,
            screen,
            retained: paint,
            font,
            layouts,
            solid_page,
        } = self;
        let mut frame = PaintFrame {
            layouts,
            font,
            metrics,
            solid_page: *solid_page,
            safe_area: SafeArea::ZERO,
            content: [400.0, 320.0],
            translate: &|_| None,
            language: [0; 3],
            now,
            nodes: &mut nodes,
            next: &mut next,
            renderers: &mut [reporter],
        };
        let context = Context::desktop();
        let at = |root| (root, px, [0; 3]);
        if retained {
            engine
                .draw_retained(
                    paint,
                    reuse,
                    ScreenArt::default(),
                    &mut frame,
                    |env, root| {
                        screen.render(
                            "fixture.root",
                            catalog,
                            &context,
                            Arc::clone(data),
                            at(root),
                            env,
                        )
                    },
                )
                .unwrap();
        } else {
            engine
                .draw(ScreenArt::default(), &mut frame, |env, root| {
                    screen.render(
                        "fixture.root",
                        catalog,
                        &context,
                        Arc::clone(data),
                        at(root),
                        env,
                    )
                })
                .unwrap();
        }
        engine.animator().end_frame();
        (nodes, next)
    }
}

/// Settled, unchanged frames reuse the paint and match a full one; each kind of change paints
/// exactly once before reuse resumes.
#[test]
fn unchanged_settled_frames_reuse_the_paint_and_each_change_paints_once() {
    let mut fixture = Fixture::new();
    for now in [0.0, 0.5] {
        fixture.frame(now, true, true);
    }
    assert_eq!(
        fixture.retained.paints, 2,
        "a running fade paints every frame"
    );
    let settled = fixture.frame(1.5, true, true);
    assert!(!settled.0.is_empty());
    assert_eq!(fixture.retained.paints, 3);
    for now in [2.0, 30.0] {
        assert_eq!(fixture.frame(now, true, true), settled);
    }
    assert_eq!(fixture.retained.paints, 3, "settled frames reuse the paint");
    assert_eq!(
        fixture.frame(31.0, true, false),
        settled,
        "a full paint after many reused frames draws the same nodes"
    );
    fixture.frame(32.0, false, true);
    assert_eq!(fixture.retained.paints, 4, "a caller change paints");

    let mut now = 33.0;
    let mut changed = |fixture: &mut Fixture, change: &str| {
        let paints = fixture.retained.paints;
        fixture.frame(now, true, true);
        assert_eq!(fixture.retained.paints, paints + 1, "{change} paints once");
        fixture.frame(now + 1.0, true, true);
        assert_eq!(
            fixture.retained.paints,
            paints + 1,
            "{change} is then reused"
        );
        now += 2.0;
    };
    fixture.solid_page = 1;
    changed(&mut fixture, "a frame input");
    fixture.engine.textures.set_full_res(HashMap::new());
    changed(&mut fixture, "a texture source");
    fixture.engine.animator().fire("button.fixture");
    changed(&mut fixture, "a fired event");
    fixture.screen.invalidate_textures();
    changed(&mut fixture, "a new layout");
}

/// A replayed screen reuses the atlas keys it resolved and still requires them each frame;
/// changed renderer paths, texture sources or layout resolve them again.
#[test]
fn replayed_frames_require_resolved_keys_and_resolve_again_after_changes() {
    let mut png = Vec::new();
    image::RgbaImage::from_pixel(16, 16, image::Rgba([255; 4]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let atlas = || {
        ServerAtlas::new(
            &[
                ("textures/ui/a.png".into(), png.clone()),
                ("textures/ui/b.png".into(), png.clone()),
            ],
            None,
            2,
        )
    };
    let resident =
        |fixture: &Fixture, key: &str| fixture.engine.textures.lock().placement(key).is_some();
    let mut fixture = Fixture::new();
    fixture.engine.textures.set_atlas(atlas(), 8);
    fixture.reporter.0 = vec!["textures/ui/a"];
    for now in [0.0, 0.5, 1.5] {
        fixture.frame(now, true, true);
    }
    assert!(resident(&fixture, "textures/ui/a"));
    let (paints, resolutions) = (
        fixture.retained.paints,
        fixture.retained.required.resolutions,
    );
    for now in [2.0, 3.0, 4.0] {
        fixture.frame(now, true, true);
    }
    assert_eq!(fixture.retained.paints, paints, "settled frames replay");
    assert_eq!(
        fixture.retained.required.resolutions, resolutions,
        "replayed frames reuse their resolved keys"
    );
    assert!(resident(&fixture, "textures/ui/a"));

    let mut now = 5.0;
    let mut changed = |fixture: &mut Fixture, change: &str, key: &str| {
        let resolutions = fixture.retained.required.resolutions;
        fixture.frame(now, true, true);
        assert!(
            fixture.retained.required.resolutions > resolutions,
            "{change} resolves its keys"
        );
        assert!(resident(fixture, key), "{change} requires {key}");
        // Placing a newly required key changes the atlas once more.
        fixture.frame(now + 1.0, true, true);
        let resolutions = fixture.retained.required.resolutions;
        for later in [2.0, 3.0] {
            fixture.frame(now + later, true, true);
        }
        assert_eq!(
            fixture.retained.required.resolutions, resolutions,
            "{change} then reuses its keys"
        );
        assert!(resident(fixture, key));
        now += 4.0;
    };
    fixture.reporter.0 = vec!["textures/ui/b"];
    changed(&mut fixture, "a renderer path", "textures/ui/b");
    fixture.engine.textures.set_atlas(atlas(), 8);
    changed(&mut fixture, "a new atlas", "textures/ui/b");
    fixture.screen.invalidate_textures();
    changed(&mut fixture, "a new layout", "textures/ui/b");
}
