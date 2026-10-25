//! Mod annotation spans share the eligible name's font atlas and native depth passes.

use super::super::nametag_atlas::AtlasLine;
use super::*;
use std::sync::LazyLock;
use ui::mod_presentation::{NametagStyle, SpanPosition, Spans};

static SPACE: LazyLock<Arc<str>> = LazyLock::new(|| Arc::from(" "));

/// Pins span glyphs, and the gap beside the name, before this frame captures any atlas UVs.
pub(super) fn texts(spans: &Spans) -> impl Iterator<Item = &Arc<str>> {
    spans.texts().chain(std::iter::once(&*SPACE))
}

/// Reserves one coloured run per span and, for spans on their own line, its backing plate.
pub(super) fn record_budget(spans: &Spans, style: Option<&NametagStyle>) -> usize {
    spans.spans.len() * style::record_cost(style, false)
        + usize::from(own_line(spans.position)) * style::record_cost(style, true)
}

/// Whether spans take their own line, above or below the name, instead of extending its first
/// line.
pub(super) fn separate(spans: &Spans) -> bool {
    own_line(spans.position)
}

/// Whether spans precede the name, on its first line or on the line above.
pub(super) fn before(spans: &Spans) -> bool {
    leads(spans.position)
}

/// Whether spans at `position` take their own line instead of extending the name's first line.
const fn own_line(position: SpanPosition) -> bool {
    matches!(position, SpanPosition::Above | SpanPosition::Below)
}

/// Whether spans at `position` precede the name, on its first line or on the line above.
const fn leads(position: SpanPosition) -> bool {
    matches!(position, SpanPosition::Before | SpanPosition::Above)
}

pub(super) struct Layout {
    runs: Vec<(AtlasLine, [f32; 4])>,
    position: SpanPosition,
    pub(super) gap: f32,
}

impl Layout {
    /// Resolves cached cells; exhaustion omits the spans without displacing their name.
    pub(super) fn measure<'p>(
        spans: &Spans,
        atlas: &mut NametagAtlas,
        font: &RuntimeFontCatalog,
        layouts: &mut TextLayoutCache,
        pages: &impl Fn(usize) -> Option<GlyphPage<'p>>,
    ) -> Option<Self> {
        let runs = spans
            .spans
            .iter()
            .map(|span| Some((atlas.line(&span.text, font, layouts, pages)?, span.color)))
            .collect::<Option<_>>()?;
        Some(Self {
            runs,
            position: spans.position,
            gap: atlas.line(&SPACE, font, layouts, pages)?.width_px,
        })
    }

    /// Uses actual span advances rather than estimated character widths.
    pub(super) fn width(&self) -> f32 {
        self.runs.iter().map(|(line, _)| line.width_px).sum()
    }

    /// Spans above or below the name occupy one line instead of extending the first name line.
    pub(super) fn separate(&self) -> bool {
        own_line(self.position)
    }

    /// Selects placement before the name or its multiline block.
    pub(super) fn before(&self) -> bool {
        leads(self.position)
    }

    /// Emits independently tinted runs before the existing depth/alpha pass construction.
    pub(super) fn append(
        &self,
        records: &mut Vec<NametagRecord>,
        common: NametagRecord,
        mut left: f32,
        index: usize,
        alpha: f32,
    ) {
        let side = NAMETAG_ATLAS_SIDE as f32;
        let texels = FONT_DESIGN_PIXEL_TEXELS as f32;
        for (line, rgba) in &self.runs {
            let [x, y, width, height] = line.cell.map(|v| v as f32);
            let top = NAMETAG_LINE_PITCH_PX * index as f32 + line.top_px;
            let mut color = *rgba;
            color[3] *= alpha;
            records.push(NametagRecord {
                text: render_model::NAMETAG_ANNOTATION_TEXT,
                rect: [left, top, left + width / texels, top + height / texels],
                uv: [x / side, y / side, (x + width) / side, (y + height) / side],
                color,
                ..common
            });
            left += line.width_px;
        }
    }
}
