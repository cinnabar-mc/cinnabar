//! Hover lines a firework rocket or firework star adds below its name.

use protocol::FireworkExplosion;

use super::{BLUE_TEXT_COLOR, TooltipLine, UiRuntime};

/// Lang key suffixes of the sixteen dye colours, indexed by their stored colour value.
const COLOR_KEYS: [&str; 16] = [
    "black",
    "red",
    "green",
    "brown",
    "blue",
    "purple",
    "cyan",
    "silver",
    "gray",
    "pink",
    "lime",
    "yellow",
    "lightBlue",
    "magenta",
    "orange",
    "white",
];

/// Explosion shapes with their own `item.fireworksCharge.type.N` name.
const NAMED_SHAPES: u8 = 5;

/// Lines for a rocket's flight duration and each explosion, or a star's explosion.
///
/// The lines end once their text reaches `ui::UiLimits::MAX_TEXT_BYTES`: the hover label
/// lays out only that many bytes, so later text is never shown, and stopping there keeps
/// many colours with long pack translations from expanding without bound.
pub(super) fn lines(runtime: &UiRuntime, identifier: &str, extra_data: &[u8]) -> Vec<TooltipLine> {
    let mut lines = Lines::new(runtime);
    if identifier == assets::facts::FIREWORK_ROCKET_IDENTIFIER {
        if let Some(fireworks) = protocol::item_fireworks(extra_data) {
            if let Some(flight) = fireworks.flight {
                let mut text = lines.translate("item.fireworks.flight");
                text.push_str(&format!(" {flight}"));
                lines.push(text);
            }
            for explosion in &fireworks.explosions {
                if lines.is_full() {
                    break;
                }
                lines.explosion(explosion);
            }
        }
    } else if identifier == assets::facts::FIREWORK_STAR_IDENTIFIER
        && let Some(explosion) = protocol::item_firework_star(extra_data)
    {
        lines.explosion(&explosion);
    }
    lines
        .texts
        .into_iter()
        .map(|text| TooltipLine {
            text,
            color: BLUE_TEXT_COLOR,
        })
        .collect()
}

/// Firework hover lines built within the bytes the hover label can show.
struct Lines<'a> {
    runtime: &'a UiRuntime,
    texts: Vec<String>,
    remaining: usize,
}

impl<'a> Lines<'a> {
    fn new(runtime: &'a UiRuntime) -> Self {
        Self {
            runtime,
            texts: Vec::new(),
            remaining: ui::UiLimits::MAX_TEXT_BYTES,
        }
    }

    /// Whether earlier lines already fill every byte the label lays out.
    fn is_full(&self) -> bool {
        self.remaining == 0
    }

    /// Adds a line unless the label is full, charging its bytes and line break.
    fn push(&mut self, text: String) {
        if self.is_full() {
            return;
        }
        self.remaining = self.remaining.saturating_sub(text.len() + 1);
        self.texts.push(text);
    }

    /// Adds an explosion's shape, colours, fade colours, trail and twinkle lines.
    fn explosion(&mut self, explosion: &FireworkExplosion) {
        let shape = if explosion.shape < NAMED_SHAPES {
            self.translate(&format!("item.fireworksCharge.type.{}", explosion.shape))
        } else {
            self.translate("item.fireworksCharge.type")
        };
        self.push(shape);
        if !explosion.colors.is_empty() {
            self.push_colors(String::new(), &explosion.colors);
        }
        if !explosion.fade_colors.is_empty() {
            let mut text = self.translate("item.fireworksCharge.fadeTo");
            text.push(' ');
            self.push_colors(text, &explosion.fade_colors);
        }
        if explosion.trail {
            self.push(self.translate("item.fireworksCharge.trail"));
        }
        if explosion.flicker {
            self.push(self.translate("item.fireworksCharge.flicker"));
        }
    }

    /// Adds `text` followed by each colour's name, joined by commas; values past the dye
    /// range read as custom. Names stop once the line alone fills the label.
    fn push_colors(&mut self, mut text: String, colors: &[u8]) {
        for (index, color) in colors.iter().enumerate() {
            if text.len() >= self.remaining {
                break;
            }
            if index > 0 {
                text.push_str(", ");
            }
            match COLOR_KEYS.get(usize::from(*color)) {
                Some(key) => self.append(&mut text, &format!("item.fireworksCharge.{key}")),
                None => self.append(&mut text, "item.fireworksCharge.customColor"),
            }
        }
        self.push(text);
    }

    /// The key's current-language text, or the key itself when no pack defines it.
    fn translate(&self, key: &str) -> String {
        let mut text = String::new();
        self.append(&mut text, key);
        text
    }

    /// Appends the key's current-language text, or the key itself when no pack defines it.
    fn append(&self, text: &mut String, key: &str) {
        match self.runtime.translation(key) {
            Some(translation) => text.push_str(&translation),
            None => text.push_str(key),
        }
    }
}

#[cfg(test)]
mod tests;
