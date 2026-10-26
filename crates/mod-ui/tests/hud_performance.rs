//! Repeatable HUD surface benchmark through the production engine; no game resources needed.
//! Run with `--nocapture` to read the `HUD_SURFACE_BENCH` lines.

use std::{hint::black_box, sync::Arc, time::Instant};

use json_ui::Context;
use mod_ui::{ActorAuthority, ModUi, PanelUiOwner, PublishedHudSurface};
use serde_json::json;
use ui::mod_panel::Surface;
use ui_engine::{
    engine::{
        FormEngine,
        screen::{PaintFrame, ScreenRegistry},
    },
    sources::{ImageWork, RemoteImage, RemoteSource},
    text_metrics::TextMetrics,
};

#[global_allocator]
static ALLOCATOR: test_alloc::CountingAllocator = test_alloc::CountingAllocator;

const OWNER: PanelUiOwner = PanelUiOwner::Environment(0);
const ROWS: usize = 8;

struct NoImages;
impl RemoteSource for NoImages {
    fn state(&self, _: &str) -> RemoteImage {
        RemoteImage::Failed
    }
}

/// `cards` cards of eight label and bound value rows over a rounded background, as a
/// first-party HUD of status cards draws them, sharing base controls as authored HUDs do;
/// value 0 of card 0 is `revision`.
fn surface(cards: usize, revision: usize) -> Surface {
    surface_with(cards, cards, revision)
}

/// [`surface`] with only the first `bound` cards' values bound; the rest are literal text.
fn surface_with(cards: usize, bound: usize, revision: usize) -> Surface {
    let count = cards;
    let cards: Vec<_> = (0..count)
        .map(|card| {
            let rows: Vec<_> = (0..ROWS)
                .flat_map(|row| {
                    let value = format!("#c{card}_r{row}");
                    [
                        json!({format!("l{row}@bench.l"):{"text":format!("Row {row}"),
                            "offset":[4, 4 + row * 10]}}),
                        if card < bound {
                            json!({format!("v{row}@bench.l"):{"text":value,
                                "offset":[80, 4 + row * 10],"bindings":[{"binding_name":value}]}})
                        } else {
                            json!({format!("v{row}@bench.l"):{"text":"1234",
                                "offset":[80, 4 + row * 10]}})
                        },
                    ]
                })
                .collect();
            let mut controls = vec![json!({"bg@bench.bg":{}})];
            controls.extend(rows);
            json!({format!("card_{card}@bench.card"):{
                "offset":[(card % 8) * 130, (card / 8) * 90],"controls":controls}})
        })
        .collect();
    let mut bindings = serde_json::Map::new();
    for card in 0..bound {
        for row in 0..ROWS {
            let value = if card == 0 && row == 0 {
                revision.to_string()
            } else {
                "1234".into()
            };
            bindings.insert(format!("#c{card}_r{row}"), json!(value));
        }
    }
    let size = [120, 4 + ROWS * 10];
    serde_json::from_value(json!({"screen":"bench.root",
        "document":json!({"namespace":"bench",
            "l":{"type":"label","anchor_from":"top_left","anchor_to":"top_left"},
            "bg":{"type":"custom","renderer":"cinnabar_rounded_rectangle","size":size,
                "radius":3,"color":[0,0,0,0.6]},
            "card":{"type":"panel","size":size,"anchor_from":"top_left","anchor_to":"top_left"},
            "root":{"type":"panel","size":["100%","100%"],"controls":cards}}).to_string(),
        "bindings":bindings}))
    .unwrap()
}

struct Harness {
    engine: FormEngine,
    font: Arc<assets::RuntimeFontCatalog>,
    layouts: ui::TextLayoutCache,
    nodes: Vec<ui::UiNode>,
}

impl Harness {
    fn new() -> Self {
        static REGISTRY: std::sync::OnceLock<ScreenRegistry> = std::sync::OnceLock::new();
        let bytes = assets::encode_ui_catalog([1; 32], &[], &[], &[], &[]).unwrap();
        Self {
            engine: FormEngine::new(
                Arc::new(assets::RuntimeUiAssets::decode(&bytes).unwrap()),
                json_ui::Catalog::default(),
                0,
                REGISTRY.get_or_init(|| ScreenRegistry::new(Context::default())),
                Arc::new(NoImages),
                ImageWork::inline(),
            ),
            font: ui_engine::test_support::fixture_font(),
            layouts: ui::TextLayoutCache::new(4096, 16 * 1024 * 1024),
            nodes: Vec::new(),
        }
    }

    fn paint(&mut self, ui: &mut ModUi) {
        self.nodes.clear();
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
        ui.paint_hud_surfaces(Some(&self.engine), &mut frame, &|_, _| None);
    }
}

fn publish(ui: &mut ModUi, revision: u64, surface: &Arc<Surface>) {
    ui.set_hud_surfaces(std::iter::once(PublishedHudSurface {
        owner: OWNER,
        revision,
        surface,
        authority: ActorAuthority::default(),
    }));
}

/// Reports warm timings and allocator requests of `work`.
#[expect(
    clippy::print_stdout,
    reason = "the opt-in benchmark reports measurements"
)]
fn measure(label: &str, mut work: impl FnMut()) {
    for _ in 0..32 {
        work();
    }
    let (_, allocation) = test_alloc::measure(&mut work);
    let mut times = Vec::with_capacity(512);
    for _ in 0..512 {
        let start = Instant::now();
        work();
        times.push(start.elapsed().as_nanos());
    }
    times.sort_unstable();
    println!(
        "HUD_SURFACE_BENCH {label} ns_p50={} ns_p99={} allocations={} bytes={}",
        times[times.len() / 2],
        times[times.len() * 99 / 100],
        allocation.calls,
        allocation.bytes
    );
}

/// Steady frames replay cached controls; one changed value repaints one card. 64 cards bind
/// the 512 values a HUD surface may retain.
#[test]
fn benchmark_hud_surface_frames() {
    for cards in [16, 64] {
        benchmark(cards);
    }
    // Admission grows with the document: measure it near the old and the new byte limits.
    for cards in [100, 200] {
        let surface = surface_with(cards, 64, 0);
        let json = serde_json::to_string(&surface).unwrap();
        println!(
            "HUD_SURFACE_BENCH document_{cards}_cards bytes={} json_bytes={}",
            surface.document.len(),
            json.len()
        );
        surface.validate_hud().unwrap();
        measure(&format!("host_admission_{cards}_cards"), || {
            let surface: Surface = serde_json::from_str(black_box(&json)).unwrap();
            surface.validate_hud().unwrap();
        });
    }
}

fn benchmark(cards: usize) {
    let mut harness = Harness::new();
    let mut ui = ModUi::default();
    let surfaces: Vec<Arc<Surface>> = (0..2).map(|i| Arc::new(surface(cards, i))).collect();
    surfaces[0].validate_hud().unwrap();
    let cold = Instant::now();
    publish(&mut ui, 1, &surfaces[0]);
    harness.paint(&mut ui);
    #[expect(
        clippy::print_stdout,
        reason = "the opt-in benchmark reports measurements"
    )]
    {
        println!(
            "HUD_SURFACE_BENCH first_paint_{cards}_cards ns={}",
            cold.elapsed().as_nanos()
        );
    }
    measure(&format!("steady_{cards}_cards"), || {
        harness.paint(black_box(&mut ui));
    });
    let mut revision = 1;
    measure(&format!("one_value_changed_{cards}_cards"), || {
        revision += 1;
        publish(&mut ui, revision, &surfaces[(revision % 2) as usize]);
        harness.paint(&mut ui);
    });
    measure(&format!("publish_only_{cards}_cards"), || {
        revision += 1;
        publish(&mut ui, revision, &surfaces[(revision % 2) as usize]);
    });
    let patch = r##"{"binding_changes":{"#c0_r0":"7"}}"##;
    measure(&format!("host_patch_{cards}_cards"), || {
        let patched = ui::mod_panel::SurfacePatch::parse(black_box(patch))
            .unwrap()
            .apply(&surfaces[0], ui::mod_panel::SurfaceRole::Hud)
            .unwrap();
        black_box(Arc::new(patched));
    });
    let json = serde_json::to_string(surfaces[0].as_ref()).unwrap();
    measure(&format!("host_admission_{cards}_cards"), || {
        let surface: Surface = serde_json::from_str(black_box(&json)).unwrap();
        surface.validate_hud().unwrap();
    });
}
