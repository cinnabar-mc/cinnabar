//! Rasterizes sign text into the 96x48 canvas the renderer maps onto a board face.
//!
//! The canvas is in Mojang design pixels (two font texels each). Bedrock wraps each source
//! line at its board's text width and draws the first four visual lines. Line pitch and the
//! glow outline treatment still follow the Java Edition sign and need native measurement.

use assets::CompiledFontCatalog;
use render_model::TEXT_CELL;
use std::hash::Hash;
use std::hash::Hasher;
use ui::{TextLayoutCache, TextLayoutRequest, TextLineAlign, TextStyle, TextWrap, UiScale};

const LINES: usize = 4;
const LINE_PITCH_PIXELS: i32 = 10;
/// Widest line a standing sign holds before wrapping.
pub(super) const STANDING_WRAP_PIXELS: u32 = 90;
/// Widest line a hanging sign holds before wrapping; its board is narrower.
pub(super) const HANGING_WRAP_PIXELS: u32 = 60;
const TEXELS_PER_PIXEL: u32 = 2;
/// Wrap width wide enough that a measured line never wraps.
const WIDTH_PROBE_PIXELS: u32 = 4_096;
const ASCENT_TEXELS: u32 = 14;
const LINE_HEIGHT_TEXELS: u32 = 18;
const BLACK_OUTLINE_RGB: [u8; 3] = [0xF0, 0xEB, 0xCC];

/// One face's text as stored in the block entity, with its canvas cache key computed once
/// because every drawn sign looks its canvas up each frame.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct SignTextSpec {
    text: String,
    /// `SignTextColor` as ARGB; alpha is ignored.
    color_argb: i32,
    glowing: bool,
    hide_glow_outline: bool,
    /// Widest line before Bedrock wraps it, in design pixels.
    wrap_width_pixels: u32,
    /// Hash of every field above; equal specs share one canvas.
    key: u64,
}

impl SignTextSpec {
    /// A face showing `text` in `SignTextColor` `color_argb`, glowing or not.
    pub(super) fn new(
        text: String,
        color_argb: i32,
        glowing: bool,
        hide_glow_outline: bool,
        wrap_width_pixels: u32,
    ) -> Self {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        text.hash(&mut hasher);
        color_argb.hash(&mut hasher);
        glowing.hash(&mut hasher);
        hide_glow_outline.hash(&mut hasher);
        wrap_width_pixels.hash(&mut hasher);
        Self {
            text,
            color_argb,
            glowing,
            hide_glow_outline,
            wrap_width_pixels,
            key: hasher.finish(),
        }
    }

    /// Whether the text ignores world light.
    pub(super) const fn glowing(&self) -> bool {
        self.glowing
    }

    /// Whether the face draws anything.
    pub(super) fn is_visible(&self) -> bool {
        self.text
            .chars()
            .any(|character| !character.is_whitespace())
    }

    /// Identifies this face's canvas; equal specs share one key.
    pub(super) const fn cache_key(&self) -> u64 {
        self.key
    }

    fn base_rgb(&self) -> [u8; 3] {
        let [_, red, green, blue] = self.color_argb.to_be_bytes();
        [red, green, blue]
    }

    fn outline_rgb(&self) -> [u8; 3] {
        let [red, green, blue] = self.base_rgb();
        if red == 0 && green == 0 && blue == 0 {
            BLACK_OUTLINE_RGB
        } else {
            // A darkened copy of the text color.
            [red, green, blue].map(|channel| (u16::from(channel) * 2 / 5) as u8)
        }
    }
}

/// Width of `text` in design pixels as one unwrapped line, or `None` if the font cannot lay it out.
pub(crate) fn line_width_design_pixels(
    text: &str,
    font: &CompiledFontCatalog,
    layouts: &mut TextLayoutCache,
) -> Option<f32> {
    let layout = layouts
        .layout(TextLayoutRequest {
            text,
            style: TextStyle::default(),
            width_64: WIDTH_PROBE_PIXELS * TEXELS_PER_PIXEL * 64,
            line_height_64: LINE_HEIGHT_TEXELS * 64,
            baseline_64: ASCENT_TEXELS * 64,
            scale: UiScale::default(),
            font,
            wrap: Default::default(),
        })
        .ok()?;
    Some(layout.size_64()[0] as f32 / (TEXELS_PER_PIXEL * 64) as f32)
}

struct Placed {
    /// Destination top-left in canvas pixels.
    origin: [i32; 2],
    /// Source rect in font-page texels `[x0, y0, x1, y1]`.
    source: [u16; 4],
    page: usize,
    rgb: [u8; 3],
}

/// The wrapped, centre-aligned layout every sign face rasterizes from.
fn layout_request<'a>(
    spec: &'a SignTextSpec,
    font: &'a CompiledFontCatalog,
) -> TextLayoutRequest<'a> {
    TextLayoutRequest {
        text: &spec.text,
        style: TextStyle::default(),
        width_64: spec.wrap_width_pixels * TEXELS_PER_PIXEL * 64,
        line_height_64: LINE_HEIGHT_TEXELS * 64,
        baseline_64: ASCENT_TEXELS * 64,
        scale: UiScale::default(),
        font,
        wrap: TextWrap {
            align: TextLineAlign::Center,
            ..TextWrap::default()
        },
    }
}

/// Rasterizes `spec` into a `TEXT_CELL` RGBA8 canvas; `None` when the font cannot lay it out.
pub(super) fn rasterize(
    spec: &SignTextSpec,
    font: &CompiledFontCatalog,
    layouts: &mut TextLayoutCache,
) -> Option<Vec<u8>> {
    let [width, height] = TEXT_CELL.map(|value| value as i32);
    let Ok(layout) = layouts.layout(layout_request(spec, font)) else {
        return None;
    };
    let mut canvas = vec![0u8; (width * height * 4) as usize];
    let mut placed = Vec::new();
    let block_top = (height - LINES as i32 * LINE_PITCH_PIXELS) / 2;
    // The layout centres lines inside its wrap column; the column itself is centred on the board.
    let left = (width - spec.wrap_width_pixels as i32) / 2;
    let texel_64 = (TEXELS_PER_PIXEL * 64) as f32;
    let line_height_64 = (LINE_HEIGHT_TEXELS * 64) as i32;
    for glyph in layout.glyphs() {
        let line = i32::from(glyph.line);
        if line >= LINES as i32 {
            break;
        }
        // Glyph bounds carry the layout's own 9-pixel pitch; the board pitches lines 10 pixels apart.
        let within_line_64 = glyph.bounds_64[1] - line * line_height_64;
        placed.push(Placed {
            origin: [
                left + (glyph.bounds_64[0] as f32 / texel_64).round() as i32,
                block_top
                    + line * LINE_PITCH_PIXELS
                    + (within_line_64 as f32 / texel_64).round() as i32,
            ],
            source: glyph.uv,
            page: usize::from(glyph.page),
            rgb: glyph.style.color.rgb().unwrap_or_else(|| spec.base_rgb()),
        });
    }
    if spec.glowing && !spec.hide_glow_outline {
        let outline = spec.outline_rgb();
        for glyph in &placed {
            blit(&mut canvas, font, glyph, outline, true);
        }
    }
    for glyph in &placed {
        blit(&mut canvas, font, glyph, glyph.rgb, false);
    }
    Some(canvas)
}

/// Copies one glyph, halving the texel grid to design pixels; with `dilate`, writes the
/// eight neighbors of every inked pixel that are still empty.
fn blit(canvas: &mut [u8], font: &CompiledFontCatalog, glyph: &Placed, rgb: [u8; 3], dilate: bool) {
    let Some(page) = font.pages().get(glyph.page) else {
        return;
    };
    let [width, height] = TEXT_CELL.map(|value| value as i32);
    let [x0, y0, x1, y1] = glyph.source.map(i32::from);
    let columns = (x1 - x0 + 1) / TEXELS_PER_PIXEL as i32;
    let rows = (y1 - y0 + 1) / TEXELS_PER_PIXEL as i32;
    for row in 0..rows {
        for column in 0..columns {
            let source_x = x0 + column * TEXELS_PER_PIXEL as i32;
            let source_y = y0 + row * TEXELS_PER_PIXEL as i32;
            let Some(alpha) = page_alpha(page, source_x, source_y) else {
                continue;
            };
            if alpha == 0 {
                continue;
            }
            let (x, y) = (glyph.origin[0] + column, glyph.origin[1] + row);
            let offsets: &[(i32, i32)] = if dilate {
                &[
                    (-1, -1),
                    (0, -1),
                    (1, -1),
                    (-1, 0),
                    (1, 0),
                    (-1, 1),
                    (0, 1),
                    (1, 1),
                ]
            } else {
                &[(0, 0)]
            };
            for (dx, dy) in offsets {
                let (px, py) = (x + dx, y + dy);
                if px < 0 || py < 0 || px >= width || py >= height {
                    continue;
                }
                let index = ((py * width + px) * 4) as usize;
                if dilate && canvas[index + 3] != 0 {
                    continue;
                }
                canvas[index..index + 4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
            }
        }
    }
}

fn page_alpha(page: &assets::FontTexturePage, x: i32, y: i32) -> Option<u8> {
    let (x, y) = (u32::try_from(x).ok()?, u32::try_from(y).ok()?);
    if x >= page.width || y >= page.height {
        return None;
    }
    page.pixels
        .texel((y * page.width + x) as usize)
        .map(|texel| texel[3])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(text: &str) -> SignTextSpec {
        SignTextSpec::new(text.into(), -0x1000000, false, false, STANDING_WRAP_PIXELS)
    }

    /// Whether the canvas has ink in the band one visual line occupies.
    fn line_has_ink(canvas: &[u8], line: i32) -> bool {
        let [width, height] = TEXT_CELL.map(|value| value as i32);
        let top = (height - LINES as i32 * LINE_PITCH_PIXELS) / 2 + line * LINE_PITCH_PIXELS;
        (top..top + LINE_PITCH_PIXELS)
            .any(|y| (0..width).any(|x| canvas[((y * width + x) * 4 + 3) as usize] != 0))
    }

    /// The installed UI font, or `None` when its local carrier is absent.
    fn installed_font() -> Option<CompiledFontCatalog> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../.local/assets/compiled/ui-cinnangles-sans-v1.mcbefont");
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                eprintln!(
                    "skipping installed-font sign test: missing {}; make assets",
                    path.display()
                );
                return None;
            }
            Err(error) => panic!("read font fixture {}: {error}", path.display()),
        };
        let manifest = assets::carriers::startup::canonical_source_manifest_sha256(include_str!(
            "../../../assets/cinnangles-sans-source.json"
        ));
        Some(CompiledFontCatalog::decode(&bytes, manifest).expect("decode installed font fixture"))
    }

    /// The text of each visual line, in order.
    fn visual_lines(layout: &ui::TextLayout) -> Vec<String> {
        let mut lines = vec![String::new(); usize::from(layout.line_count())];
        for glyph in layout.glyphs() {
            lines[usize::from(glyph.line)].push(glyph.codepoint);
        }
        lines
    }

    #[test]
    fn color_decoding_ignores_alpha_and_black_outlines_are_light() {
        let face = SignTextSpec::new("x".into(), 0x00FF8040u32 as i32, false, false, 90);
        assert_eq!(face.base_rgb(), [0xFF, 0x80, 0x40]);
        assert_eq!(spec("x").outline_rgb(), BLACK_OUTLINE_RGB);
        assert_eq!(face.outline_rgb(), [102, 51, 25]);
    }

    #[test]
    fn keys_track_every_visible_input() {
        let base = spec("hello");
        assert_eq!(base.cache_key(), spec("hello").cache_key());
        let glowing = SignTextSpec::new("hello".into(), -0x1000000, true, false, 90);
        let hanging = SignTextSpec::new("hello".into(), -0x1000000, false, false, 60);
        assert_ne!(base.cache_key(), glowing.cache_key());
        assert_ne!(base.cache_key(), hanging.cache_key());
        assert_ne!(base.cache_key(), spec("world").cache_key());
        assert!(!spec("  \n \n").is_visible());
        assert!(base.is_visible());
    }

    #[test]
    fn wrapped_source_lines_draw_every_visual_line() {
        let font = ui_engine::test_support::fixture_font();
        let mut layouts = TextLayoutCache::new(4, 64 * 1024);
        let face = spec("00000 00000 00000 00000 00000");
        let canvas = rasterize(&face, &font, &mut layouts).unwrap();
        for line in 0..3 {
            assert!(
                line_has_ink(&canvas, line),
                "visual line {line} drew no ink"
            );
        }
        assert!(!line_has_ink(&canvas, 3));
    }

    #[test]
    fn the_reported_long_line_wraps_into_three_visual_lines() {
        let Some(font) = installed_font() else {
            return;
        };
        let mut layouts = TextLayoutCache::new(4, 64 * 1024);
        let face = spec("that is 100% with less lag/lag reducer");
        let layout = layouts.layout(layout_request(&face, &font)).unwrap();
        assert_eq!(
            visual_lines(&layout),
            ["that is 100% with", "less lag/lag", "reducer"]
        );
    }

    #[test]
    fn hanging_signs_wrap_at_the_narrower_board_width() {
        let font = ui_engine::test_support::fixture_font();
        let mut layouts = TextLayoutCache::new(4, 64 * 1024);
        // Fifteen fixture glyphs measure exactly 90 design pixels.
        let text = "000000000000000";
        let standing =
            SignTextSpec::new(text.into(), -0x1000000, false, false, STANDING_WRAP_PIXELS);
        let hanging = SignTextSpec::new(text.into(), -0x1000000, false, false, HANGING_WRAP_PIXELS);
        let standing_canvas = rasterize(&standing, &font, &mut layouts).unwrap();
        let hanging_canvas = rasterize(&hanging, &font, &mut layouts).unwrap();
        assert!(line_has_ink(&standing_canvas, 0));
        assert!(!line_has_ink(&standing_canvas, 1));
        assert!(line_has_ink(&hanging_canvas, 0));
        assert!(line_has_ink(&hanging_canvas, 1));
    }
}
