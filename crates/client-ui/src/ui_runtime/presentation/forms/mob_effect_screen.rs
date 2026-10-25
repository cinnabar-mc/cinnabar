//! Draws vanilla's mob effect screen from the local player's active effects.

use ui::{UiNode, UiPoint};

use super::super::{UiPresentationError, UiPresentationRuntime};
use crate::ui_runtime::UiRuntime;
use ui_engine::engine::screen::PaintFrame;
use ui_engine::frame::EngineFrame;
use ui_engine::text_metrics::TextMetrics;
use ui_screens::mob_effect_screen::effect_rows;

impl UiPresentationRuntime {
    pub(in super::super) fn append_mob_effect_screen(
        &mut self,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        content: [f32; 2],
        now_millis: u64,
    ) -> Result<(), UiPresentationError> {
        let Some(engine) = self.engine.as_deref() else {
            return Ok(());
        };
        let translate = |key: &str| runtime.translation(key);
        let rows = effect_rows(&runtime.mob_effect_entries(now_millis), &|key| {
            translate(key).map(|text| text.to_string())
        });
        let mut frame = PaintFrame {
            layouts: &mut self.layouts,
            font: &self.font,
            metrics,
            solid_page: self.solid_texture_page,
            safe_area: self.safe_area,
            content,
            translate: &translate,
            language: runtime.text_generation(),
            now: now_millis as f64 / 1_000.0,
            nodes,
            next,
            renderers: &mut [],
        };
        self.screens.mob_effects.paint(engine, &rows, &mut frame)?;
        Ok(())
    }

    /// Whether a press at `point` lands on the open mob effect screen's close button.
    pub fn mob_effect_close_hit(&self, point: UiPoint) -> bool {
        self.screens.mob_effects.close_hit(point)
    }

    /// The open screen's last painted frame, for hit regions.
    pub fn mob_effect_frame(&self) -> Option<&EngineFrame> {
        self.screens.mob_effects.frame()
    }

    pub(in super::super) fn close_mob_effect_screen(&mut self) {
        self.screens.mob_effects.close();
    }
}

#[cfg(test)]
#[path = "mob_effect_screen_tests.rs"]
mod tests;
