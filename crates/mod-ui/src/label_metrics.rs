//! Text widths exactly as a surface label lays them out, for `hud.measure-text`.

use std::{
    cell::RefCell,
    sync::{Arc, Mutex},
};

use assets::RuntimeFontCatalog;
use json_ui::{LabelShape, TextMeasure};
use ui::{FONT_DESIGN_PIXEL_TEXELS, TextLayoutCache, mod_panel::LabelFont};
use ui_engine::{engine::text_paint::Measure, text_metrics::TextMetrics};

/// Shaped lines one measurer keeps.
const CACHED_LAYOUTS: usize = 256;

/// The fonts and display metrics of the last painted HUD, with a private layout cache so
/// measuring needs no access to the presenter's frame.
pub struct LabelMetrics {
    font: Arc<RuntimeFontCatalog>,
    metrics: TextMetrics,
    layouts: Mutex<TextLayoutCache>,
}

impl LabelMetrics {
    /// Measures with `font` at the display `metrics` the HUD paints at.
    pub fn new(font: Arc<RuntimeFontCatalog>, metrics: TextMetrics) -> Self {
        Self {
            font,
            metrics,
            layouts: Mutex::new(TextLayoutCache::new(CACHED_LAYOUTS, 1 << 20)),
        }
    }

    /// Whether this measurer already uses `font` at `metrics`.
    pub fn matches(&self, font: &Arc<RuntimeFontCatalog>, metrics: TextMetrics) -> bool {
        Arc::ptr_eq(&self.font, font) && self.metrics == metrics
    }

    /// The width in GUI units of one unwrapped line at text scale 1 in `font`, as a label
    /// naming that face lays it out. Text is not localized.
    pub fn width(&self, text: &str, font: LabelFont) -> Option<f32> {
        let mut layouts = self.layouts.lock().ok()?;
        let cell = RefCell::new(&mut *layouts);
        let measure = Measure {
            layouts: &cell,
            font: &self.font,
            metrics: self.metrics,
            px: self.metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32,
            translate: &|_| None,
        };
        let shape = LabelShape {
            scale: 1.,
            line_padding: 0.,
            hide_hyphen: false,
        };
        let [width, _] = measure.named_label(text, font.name(), None, None, shape);
        Some(width as f32).filter(|width| width.is_finite())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths_grow_with_text_and_differ_between_faces() {
        let font = crate::fonts::install(ui_engine::test_support::fixture_font()).unwrap();
        let metrics =
            TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.).unwrap(), Some(2));
        let label = LabelMetrics::new(Arc::clone(&font), metrics);
        assert!(label.matches(&font, metrics));
        let short = label.width("Armor", LabelFont::Native).unwrap();
        let long = label.width("Armor 100%", LabelFont::Native).unwrap();
        assert!(short > 0. && long > short, "{short} {long}");
        assert_eq!(label.width("", LabelFont::Native), Some(0.));
        let sans = label.width("Armor 100%", LabelFont::Sans).unwrap();
        assert!(sans > 0. && sans != long, "{sans} {long}");
        assert_eq!(
            label.width("Armor", LabelFont::Native),
            Some(short),
            "cached shaping"
        );
    }
}
