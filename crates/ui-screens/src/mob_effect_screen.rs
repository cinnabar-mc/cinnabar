//! Vanilla's mob effect screen (`mob_effect.mob_effect_screen`): the local player's active
//! effects with their names, potency and remaining time, opened by the Mob Effects binding.

use std::sync::Arc;

use json_ui::{CollectionItem, DataSource, Scalar, ViewState};
use ui::{FONT_DESIGN_PIXEL_TEXELS, UiPoint};
use ui_engine::cached_screen::CachedScreen;
use ui_engine::engine::{
    FormEngine, ScreenArt,
    screen::{PaintError, PaintFrame},
};
use ui_engine::frame::EngineFrame;

use crate::regions::window_rect;

pub const MOB_EFFECT_SCREEN: &str = "mob_effect.mob_effect_screen";
/// The button the screen's close button presses and its key mappings route to.
const CLOSE_BUTTON: &str = "button.menu_continue";
const TICKS_PER_SECOND: u64 = 20;
/// Potency numerals the language file names, `potion.potency.1` (II) to `.5` (VI).
const NAMED_POTENCIES: std::ops::RangeInclusive<i32> = 1..=5;

/// One active effect of the local player.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MobEffectEntry {
    pub effect_id: i32,
    pub amplifier: i32,
    pub ambient: bool,
    /// Ticks left; `None` for an infinite effect.
    pub remaining_ticks: Option<u64>,
}

/// One row of the screen's effect grid.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MobEffectRow {
    pub name: String,
    pub timer: String,
    pub texture: String,
    pub ambient: bool,
}

/// The screen's cached layout and last frame.
#[derive(Default)]
pub struct MobEffectScreen {
    screen: CachedScreen,
    frame: Option<EngineFrame>,
}

impl MobEffectScreen {
    /// Paints the effect list over the world.
    pub fn paint(
        &mut self,
        engine: &FormEngine,
        rows: &[MobEffectRow],
        frame: &mut PaintFrame<'_>,
    ) -> Result<(), PaintError> {
        let data = screen_data(rows);
        let context = engine.context().clone();
        let catalog = Arc::clone(engine.catalog());
        let px = frame.metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        let language = frame.language;
        let view = ViewState::default();
        let screen = &mut self.screen;
        self.frame = engine.draw(ScreenArt::default(), frame, |env, root| {
            screen.render_with(
                MOB_EFFECT_SCREEN,
                &catalog,
                &context,
                data,
                (root, px, language),
                env,
                &view,
            )
        })?;
        Ok(())
    }

    /// Whether a press at `point` lands on the last painted close button.
    pub fn close_hit(&self, point: UiPoint) -> bool {
        self.frame.as_ref().is_some_and(|frame| {
            frame.hits.iter().any(|region| {
                region.enabled
                    && region.pressed.as_deref() == Some(CLOSE_BUTTON)
                    && window_rect(region, frame.scale, frame.origin)
                        .is_some_and(|bounds| bounds.contains(point))
            })
        })
    }

    pub fn frame(&self) -> Option<&EngineFrame> {
        self.frame.as_ref()
    }

    pub fn close(&mut self) {
        self.frame = None;
    }
}

/// Lists `effects` as `MobEffectScreenController::tick` does: effects with a persistent icon and
/// time left, in effect-id order, named through `translate`.
pub fn effect_rows(
    effects: &[MobEffectEntry],
    translate: &dyn Fn(&str) -> Option<String>,
) -> Vec<MobEffectRow> {
    let mut listed: Vec<_> = effects
        .iter()
        .filter(|effect| effect.remaining_ticks != Some(0))
        .filter_map(|effect| {
            let descriptor = assets::effect_descriptor(effect.effect_id)?;
            Some((effect, descriptor, descriptor.icon_path?))
        })
        .collect();
    listed.sort_by_key(|(effect, ..)| effect.effect_id);
    let text = |key: &str| translate(key).unwrap_or_else(|| key.to_owned());
    listed
        .into_iter()
        .map(|(effect, descriptor, icon)| {
            let mut name = text(descriptor.name_key);
            if NAMED_POTENCIES.contains(&effect.amplifier) {
                name.push(' ');
                name.push_str(&text(&format!("potion.potency.{}", effect.amplifier)));
            }
            MobEffectRow {
                name,
                timer: effect
                    .remaining_ticks
                    .map_or_else(|| text("effect.duration.infinite"), format_tick_duration),
                texture: icon.strip_suffix(".png").unwrap_or(icon).to_owned(),
                ambient: effect.ambient,
            }
        })
        .collect()
}

/// Formats ticks as whole minutes and two-digit seconds, `M:SS`, as vanilla's timer does.
pub fn format_tick_duration(ticks: u64) -> String {
    let seconds = ticks / TICKS_PER_SECOND;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// Binds the effect collection and the grid's one-column size. The base screen's bundle
/// tooltip shows only over container screens, which this is not.
fn screen_data(rows: &[MobEffectRow]) -> DataSource {
    let mut data = DataSource::new();
    data.set_global(
        json_ui::vanilla::Binding::IsContainerScreen.as_str(),
        Scalar::Bool(false),
    );
    let count = u32::try_from(rows.len()).unwrap_or(u32::MAX);
    let dimensions = if count == 0 { [0, 0] } else { [1, count] };
    data.set_grid_dimensions("#mob_effect_grid_size", dimensions);
    data.set_collection(
        "mob_effects_collection",
        rows.iter()
            .map(|row| {
                CollectionItem::default()
                    .with("#mob_effect_name", Scalar::Text(row.name.clone()))
                    .with("#mob_effect_timer", Scalar::Text(row.timer.clone()))
                    .with("#texture", Scalar::Text(row.texture.clone()))
                    .with("#mob_effect_is_ambient", Scalar::Bool(row.ambient))
            })
            .collect(),
    );
    data
}

#[cfg(test)]
mod tests;
