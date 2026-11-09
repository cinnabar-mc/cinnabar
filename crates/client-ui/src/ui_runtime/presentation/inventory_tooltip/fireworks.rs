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
pub(super) fn lines(runtime: &UiRuntime, identifier: &str, extra_data: &[u8]) -> Vec<TooltipLine> {
    let mut texts = Vec::new();
    if identifier == assets::facts::FIREWORK_ROCKET_IDENTIFIER {
        if let Some(fireworks) = protocol::item_fireworks(extra_data) {
            if let Some(flight) = fireworks.flight {
                texts.push(format!(
                    "{} {flight}",
                    translate(runtime, "item.fireworks.flight")
                ));
            }
            for explosion in &fireworks.explosions {
                explosion_texts(runtime, explosion, &mut texts);
            }
        }
    } else if identifier == assets::facts::FIREWORK_STAR_IDENTIFIER
        && let Some(explosion) = protocol::item_firework_star(extra_data)
    {
        explosion_texts(runtime, &explosion, &mut texts);
    }
    texts
        .into_iter()
        .map(|text| TooltipLine {
            text,
            color: BLUE_TEXT_COLOR,
        })
        .collect()
}

/// Appends an explosion's shape, colours, fade colours, trail and twinkle lines.
fn explosion_texts(runtime: &UiRuntime, explosion: &FireworkExplosion, texts: &mut Vec<String>) {
    texts.push(if explosion.shape < NAMED_SHAPES {
        translate(
            runtime,
            &format!("item.fireworksCharge.type.{}", explosion.shape),
        )
    } else {
        translate(runtime, "item.fireworksCharge.type")
    });
    if !explosion.colors.is_empty() {
        texts.push(color_list(runtime, &explosion.colors));
    }
    if !explosion.fade_colors.is_empty() {
        texts.push(format!(
            "{} {}",
            translate(runtime, "item.fireworksCharge.fadeTo"),
            color_list(runtime, &explosion.fade_colors)
        ));
    }
    if explosion.trail {
        texts.push(translate(runtime, "item.fireworksCharge.trail"));
    }
    if explosion.flicker {
        texts.push(translate(runtime, "item.fireworksCharge.flicker"));
    }
}

/// Names each stored colour, joined by commas; values past the dye range read as custom.
fn color_list(runtime: &UiRuntime, colors: &[u8]) -> String {
    colors
        .iter()
        .map(|color| match COLOR_KEYS.get(usize::from(*color)) {
            Some(key) => translate(runtime, &format!("item.fireworksCharge.{key}")),
            None => translate(runtime, "item.fireworksCharge.customColor"),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The key's current-language text, or the key itself when no pack defines it.
fn translate(runtime: &UiRuntime, key: &str) -> String {
    runtime
        .translation(key)
        .map_or_else(|| key.to_owned(), |text| text.to_string())
}

#[cfg(test)]
mod tests;
