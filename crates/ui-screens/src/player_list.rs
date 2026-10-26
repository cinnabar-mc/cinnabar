//! Momentary Java-style roster, rendered independently of server HUD replacements.

use std::{collections::BTreeMap, fmt::Write, sync::Arc};

use json_ui::{Catalog, CollectionItem, Context, DataSource, Scalar, ViewState};
use ui::FONT_DESIGN_PIXEL_TEXELS;
use ui_engine::{
    cached_screen::CachedScreen,
    engine::{
        FormEngine, ScreenArt,
        screen::{PaintError, PaintFrame},
    },
};

const MAX_PLAYERS: usize = 80;
const MAX_ROWS: usize = 20;
const ROW_HEIGHT: f64 = 9.0;
const SCREEN: &str = "cinnabar_tab.screen";
/// The roster badge's built-in texture path.
pub const CINNABAR_BADGE: &str = "textures/ui/cinnabar_badge";
/// Original gem pixels shared by tab-list and world-space name-tag atlases.
pub const CINNABAR_BADGE_PNG: &[u8] =
    include_bytes!("../../../assets/java-hud/textures/ui/cinnabar_badge.png");
/// Width a badged cell reserves beside its name.
const BADGE_WIDTH: f64 = 9.0;
/// Eight GUI pixels of face plus the gap before the name.
const HEAD_WIDTH: f64 = 9.0;

/// One roster entry as the tab list draws it; the badge belongs to this entry, not its name.
#[derive(Clone, Debug, PartialEq)]
pub struct RosterRow {
    pub name: Arc<str>,
    pub badged: bool,
    /// Mod spans beside the name; spans placed above follow it, as a row has no line above.
    pub annotation: Option<Arc<ui::mod_presentation::Spans>>,
    /// The admitted skin for this UUID; absent or unusable skins display Steve.
    pub skin: Option<render_api::StandardSkin>,
}

/// Inputs that invalidate the roster's prepared geometry and bindings.
#[derive(Clone, Copy, PartialEq)]
struct Geometry {
    root: [f64; 2],
    px: f32,
    language: [usize; 3],
    badge: bool,
    top_inset: f32,
}

/// The roster screen over its own bundled catalog, rebound only when the rows change.
pub struct PlayerList {
    catalog: Arc<Catalog>,
    screen: CachedScreen,
    rows: Vec<RosterRow>,
    total: usize,
    data: Arc<DataSource>,
    dimensions: [usize; 2],
    geometry: Option<Geometry>,
    context: Context,
    portraits: BTreeMap<String, render_model::UiTexturePage>,
    heads: Vec<String>,
}

impl PlayerList {
    fn new() -> Self {
        Self {
            catalog: Arc::new(
                Catalog::from_files([
                    ("ui/_global_variables.json", &b"{}"[..]),
                    (
                        "ui/_ui_defs.json",
                        &br#"{"ui_defs":["ui/player_list.json"]}"#[..],
                    ),
                    (
                        "ui/player_list.json",
                        &include_bytes!("../../../assets/java-hud/ui/player_list.json")[..],
                    ),
                ])
                .expect("bundled player-list JSON must be valid"),
            ),
            screen: CachedScreen::default(),
            rows: Vec::new(),
            total: 0,
            data: Arc::default(),
            dimensions: [0, 0],
            geometry: None,
            context: Context::default(),
            portraits: BTreeMap::new(),
            heads: Vec::new(),
        }
    }

    fn refresh(&mut self, players: &[RosterRow], rows: usize) -> [usize; 2] {
        let shown = &players[..players.len().min(MAX_PLAYERS)];
        let columns = shown.len().div_ceil(rows).max(1);
        let rows = shown.len().div_ceil(columns).max(1);
        if self.rows != shown || self.total != players.len() || self.dimensions != [columns, rows] {
            let mut portraits = BTreeMap::new();
            self.heads = shown
                .iter()
                .enumerate()
                .map(|(index, row)| {
                    let previous = self
                        .rows
                        .get(index)
                        .filter(|old| old.skin == row.skin)
                        .and_then(|_| self.heads.get(index))
                        .and_then(|key| self.portraits.get(key))
                        .cloned();
                    let image = previous.unwrap_or_else(|| {
                        row.skin
                            .as_ref()
                            .and_then(render_model::actor::skin_portrait::extract)
                            .unwrap_or_else(steve_portrait)
                    });
                    let key = portrait_key(&image);
                    portraits.entry(key.clone()).or_insert(image);
                    key
                })
                .collect();
            self.portraits = portraits;
            self.rows = shown.to_vec();
            self.total = players.len();
            self.dimensions = [columns, rows];
            self.geometry = None;
            let mut data = DataSource::new();
            let title = if players.len() > MAX_PLAYERS {
                format!("Online players: {} (first {MAX_PLAYERS})", players.len())
            } else {
                format!("Online players: {}", players.len())
            };
            data.set_global("#online_title", Scalar::Text(title));
            data.set_grid_dimensions("#players_dimensions", [columns as u32, rows as u32]);
            self.data = Arc::new(data);
        }
        [columns, rows]
    }

    /// Binds names and independently tinted annotation spans after the measured column width
    /// is known. Spans keep their width before the name when the column is narrow.
    fn bind_rows(&mut self, width: f64, text: &dyn json_ui::TextMeasure) {
        let [columns, rows] = self.dimensions;
        let cells = (0..rows).flat_map(|row| (0..columns).map(move |col| col * rows + row));
        let entries = cells
            .map(|index| {
                let row = self.rows.get(index);
                let annotation = row.and_then(|row| row.annotation.as_deref());
                let widths = row.map(|row| row_widths(row, text)).unwrap_or_default();
                let mut remaining = (width
                    - 8.
                    - HEAD_WIDTH
                    - if row.is_some_and(|row| row.badged) {
                        BADGE_WIDTH
                    } else {
                        0.
                    })
                .max(0.);
                let spans = widths.spans.map(|span| {
                    let span = span.min(remaining);
                    remaining -= span;
                    span
                });
                let space = widths.space.min(remaining);
                let name = widths.name.min(remaining - space);
                let before = annotation.is_some_and(|annotation| {
                    annotation.position == ui::mod_presentation::SpanPosition::Before
                });
                let spans_width: f64 = spans.iter().sum();
                let name_x = 2. + HEAD_WIDTH + if before { spans_width + space } else { 0. };
                let mut span_x = 2. + HEAD_WIDTH + if before { 0. } else { name + space };
                let mut item = CollectionItem::default()
                    .with(
                        "#player_name",
                        Scalar::Text(row.map_or_else(String::new, |row| row.name.to_string())),
                    )
                    .with("#row_visible", Scalar::Bool(row.is_some()))
                    .with(
                        "#player_head",
                        Scalar::Text(self.heads.get(index).cloned().unwrap_or_default()),
                    )
                    .with(
                        "#cinnabar_badge",
                        Scalar::Bool(row.is_some_and(|row| row.badged)),
                    )
                    .with(
                        "#name_offset",
                        Scalar::Json(serde_json::json!([name_x, 0.])),
                    )
                    .with("#name_width", Scalar::Num(name));
                for (slot, width) in spans.into_iter().enumerate() {
                    let span = annotation.and_then(|annotation| annotation.spans.get(slot));
                    let color = span.map_or([1.; 4], |span| span.color);
                    item = item
                        .with(
                            format!("#span_{slot}_text"),
                            Scalar::Text(
                                span.map_or_else(String::new, |span| span.text.to_string()),
                            ),
                        )
                        .with(
                            format!("#span_{slot}_offset"),
                            Scalar::Json(serde_json::json!([span_x, 0.])),
                        )
                        .with(format!("#span_{slot}_width"), Scalar::Num(width))
                        .with(
                            format!("#span_{slot}_color"),
                            Scalar::Json(serde_json::json!(color.map(f64::from))),
                        );
                    span_x += width;
                }
                item
            })
            .collect();
        Arc::make_mut(&mut self.data).set_collection("online_players", entries);
    }
}

/// Measured widths of one row's name, each annotation span slot and the gap between them.
#[derive(Default)]
struct RowWidths {
    name: f64,
    spans: [f64; ui::mod_presentation::MAX_ANNOTATION_SPANS],
    space: f64,
}

impl RowWidths {
    /// The unclipped width of the whole row.
    fn total(&self) -> f64 {
        self.name + self.spans.iter().sum::<f64>() + self.space
    }
}

/// Shares actual font measurements between column sizing and clipped row placement.
fn row_widths(row: &RosterRow, text: &dyn json_ui::TextMeasure) -> RowWidths {
    let mut widths = RowWidths {
        name: text.extent(&row.name)[0],
        ..Default::default()
    };
    if let Some(annotation) = &row.annotation {
        for (width, span) in widths.spans.iter_mut().zip(&annotation.spans) {
            *width = text.extent(&span.text)[0];
        }
        widths.space = text.extent(" ")[0];
    }
    widths
}

/// Atlas namespace owned by the roster's prepared portrait textures.
pub const PORTRAIT_RASTER_NAMESPACE: &str = "client-raster:tab-head/";

/// Equal portraits share one atlas allocation, including every Steve fallback row.
fn portrait_key(image: &render_model::UiTexturePage) -> String {
    let [width, height] = image.dimensions();
    let mut key = format!("{PORTRAIT_RASTER_NAMESPACE}{width}x{height}/");
    for byte in image.identity() {
        write!(key, "{byte:02x}").expect("writing into a String");
    }
    key
}

impl Default for PlayerList {
    fn default() -> Self {
        Self::new()
    }
}

impl PlayerList {
    /// Paints the roster inside a clear vertical region in logical pixels.
    pub fn paint(
        &mut self,
        engine: &FormEngine,
        players: &[RosterRow],
        frame: &mut PaintFrame<'_>,
        available_y: std::ops::Range<f32>,
    ) -> Result<(), PaintError> {
        let px = frame.metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        let language = frame.language;
        let top_inset = available_y.start.clamp(0., frame.content[1]);
        let bottom = available_y.end.clamp(top_inset, frame.content[1]);
        let max_rows = ((f64::from((bottom - top_inset) / px) - 40.0) / ROW_HEIGHT)
            .floor()
            .clamp(1.0, MAX_ROWS as f64) as usize;
        let [columns, rows] = self.refresh(players, max_rows);
        engine
            .textures
            .lock()
            .set_client_rasters(PORTRAIT_RASTER_NAMESPACE, &self.portraits);
        engine.draw(ScreenArt::default(), frame, |env, root| {
            let badge = self.rows.iter().any(|row| row.badged);
            let geometry = Geometry {
                root,
                px,
                language,
                badge,
                top_inset,
            };
            if self.geometry != Some(geometry) {
                let widest = self
                    .rows
                    .iter()
                    .map(|row| row_widths(row, env.text).total())
                    .fold(60.0, f64::max);
                let badge_width = if badge { BADGE_WIDTH } else { 0.0 };
                let width = (widest + 8.0 + HEAD_WIDTH + badge_width)
                    .min((root[0] - 8.0).max(1.0) / columns as f64);
                self.bind_rows(width, env.text);
                let title = format!("Online players: {}", self.total);
                let panel_width = (width * columns as f64 + 4.0)
                    .max(env.text.extent(&title)[0] + 4.0)
                    .min(root[0]);
                self.context = Context::default()
                    .with_var("top_inset", serde_json::json!(top_inset / px))
                    .with_var("row_height", serde_json::json!(ROW_HEIGHT))
                    .with_var("row_background_height", serde_json::json!(ROW_HEIGHT - 1.0))
                    .with_var("grid_top", serde_json::json!(ROW_HEIGHT + 3.0))
                    .with_var("cell_width", serde_json::json!(width))
                    .with_var("head_size", serde_json::json!(HEAD_WIDTH - 1.0))
                    .with_var("name_offset", serde_json::json!(2.0 + HEAD_WIDTH))
                    .with_var(
                        "name_width",
                        serde_json::json!(format!("100% - {}px", 8.0 + HEAD_WIDTH + badge_width)),
                    )
                    .with_var("cinnabar_badge_texture", serde_json::json!(CINNABAR_BADGE))
                    .with_var("panel_width", serde_json::json!(panel_width))
                    .with_var(
                        "panel_height",
                        serde_json::json!(ROW_HEIGHT + 5.0 + rows as f64 * ROW_HEIGHT),
                    );
                self.geometry = Some(geometry);
            }
            self.screen.render_shared_with(
                SCREEN,
                &self.catalog,
                &self.context,
                Arc::clone(&self.data),
                (root, px, language),
                env,
                &ViewState::default(),
            )
        })?;
        Ok(())
    }

    /// Bind+layout passes the roster screen ran.
    pub fn passes(&self) -> usize {
        self.screen.passes
    }
}

/// Uses the runtime-installed Steve texture, with the existing diagnostic skin if absent.
fn steve_portrait() -> render_model::UiTexturePage {
    static PORTRAIT: std::sync::OnceLock<render_model::UiTexturePage> = std::sync::OnceLock::new();
    PORTRAIT
        .get_or_init(|| {
            let pixels = render_model::default_actor_skin_rgba8();
            let side =
                render_model::actor_skin_side(&pixels).expect("default skin dimensions") as u32;
            render_model::actor::skin_portrait::extract(&render_api::StandardSkin {
                width: side,
                height: side,
                rgba8: pixels,
                cape: None,
                geometry: None,
            })
            .expect("default skin has a visible face")
        })
        .clone()
}

#[cfg(test)]
mod tests;
