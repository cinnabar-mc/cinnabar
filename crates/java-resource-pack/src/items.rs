//! Item textures Bedrock stores differently from Java: pre-coloured potions and the clock and
//! compass atlases.

use crate::{
    meta::Layout,
    texture::{over, paste, tint},
};
use image::RgbaImage;
use std::collections::HashMap;

/// The overlay colour Bedrock bakes into a pre-coloured potion texture, and the bottle kinds
/// Bedrock ships for that effect.
struct PotionTint {
    effect: &'static str,
    color: [u8; 3],
    drink: bool,
    splash: bool,
    lingering: bool,
}

/// Shorthand for a potion tint entry.
const fn potion(
    effect: &'static str,
    color: [u8; 3],
    drink: bool,
    splash: bool,
    lingering: bool,
) -> PotionTint {
    PotionTint {
        effect,
        color,
        drink,
        splash,
        lingering,
    }
}

/// Tints keyed by the effect suffix of `items/potion_bottle_*<effect>.png`, derived from the
/// Mojang/bedrock-samples v1.26.60.5 pre-coloured potion textures: for most effects, the bottle
/// over `potion_overlay` multiplied by the tint reproduces the Bedrock texture exactly.
const POTION_TINTS: [PotionTint; 31] = [
    potion("absorption", [60, 133, 255], true, true, false),
    potion("blindness", [71, 76, 71], true, true, false),
    potion("confusion", [138, 48, 121], true, true, false),
    potion("damageBoost", [252, 197, 0], true, true, true),
    potion("digSlowdown", [121, 107, 38], true, true, false),
    potion("digSpeed", [255, 255, 109], true, true, false),
    potion("fireResistance", [252, 151, 0], true, true, true),
    potion("harm", [167, 100, 105], true, true, true),
    potion("heal", [245, 35, 34], true, true, true),
    potion("healthBoost", [255, 203, 57], true, true, false),
    potion("hunger", [143, 191, 135], true, true, false),
    potion("infested", [139, 153, 139], true, true, true),
    potion("invisibility", [243, 243, 243], true, true, true),
    potion("jump", [250, 252, 131], true, true, true),
    potion("levitation", [250, 252, 131], true, true, false),
    potion("luck", [88, 191, 6], false, false, true),
    potion("moveSlowdown", [137, 173, 222], true, true, true),
    potion("moveSpeed", [50, 232, 252], true, true, true),
    potion("nightVision", [192, 252, 101], true, true, true),
    potion("oozing", [151, 252, 161], true, true, true),
    potion("poison", [134, 161, 98], true, true, true),
    potion("regeneration", [203, 91, 169], true, true, true),
    potion("resistance", [252, 151, 0], true, true, false),
    potion("saturation", [245, 35, 34], true, true, false),
    potion("slowFall", [243, 207, 185], true, true, true),
    potion("turtleMaster", [139, 129, 227], true, true, true),
    potion("waterBreathing", [150, 215, 190], true, true, true),
    potion("weakness", [71, 76, 71], true, true, true),
    potion("weaving", [119, 104, 89], true, true, true),
    potion("windCharged", [186, 198, 251], true, true, true),
    potion("wither", [115, 97, 86], true, true, true),
];

/// Bottle kinds: the Bedrock name infix and the Java bottle texture per layout.
const BOTTLES: [(&str, &str, &str); 3] = [
    ("", "potion_bottle_drinkable", "potion"),
    ("splash_", "potion_bottle_splash", "splash_potion"),
    ("lingering_", "potion_bottle_lingering", "lingering_potion"),
];

/// Java animation frames Bedrock stores as one vertical atlas: frame prefix, atlas name, frames.
const ITEM_ATLASES: [(&str, &str, usize); 3] = [
    ("clock_", "watch_atlas", 64),
    ("compass_", "compass_atlas", 32),
    ("recovery_compass_", "recovery_compass_atlas", 32),
];

/// Whether the potion or atlas translation reads the item texture named `name`.
pub(crate) fn composes(name: &str) -> bool {
    name == "potion_overlay"
        || BOTTLES
            .iter()
            .any(|(_, legacy, flattened)| name == *legacy || name == *flattened)
        || ITEM_ATLASES.iter().any(|(prefix, _, _)| {
            name.strip_prefix(prefix)
                .is_some_and(|frame| frame.len() == 2 && frame.bytes().all(|b| b.is_ascii_digit()))
        })
}

/// Builds Bedrock's pre-coloured potion textures. Java draws a potion as its bottle over a tinted
/// `potion_overlay`, while Bedrock ships one texture per effect, so each is composed the same way.
/// Nothing is produced for a bottle kind unless the pack overrides both that bottle and the overlay.
pub(crate) fn potion_textures(
    items: &HashMap<String, RgbaImage>,
    layout: Layout,
) -> Vec<(String, RgbaImage)> {
    let Some(overlay) = items.get("potion_overlay") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (kind, legacy, flattened) in BOTTLES {
        let bottle_name = match layout {
            Layout::Legacy => legacy,
            Layout::Flattened => flattened,
        };
        let Some(bottle) = items
            .get(bottle_name)
            .filter(|bottle| bottle.dimensions() == overlay.dimensions())
        else {
            continue;
        };
        for tint_entry in &POTION_TINTS {
            let shipped = match kind {
                "" => tint_entry.drink,
                "splash_" => tint_entry.splash,
                _ => tint_entry.lingering,
            };
            if shipped {
                out.push((
                    format!(
                        "textures/items/potion_bottle_{kind}{}.png",
                        tint_entry.effect
                    ),
                    over(&tint(overlay, tint_entry.color), bottle),
                ));
            }
        }
    }
    out
}

/// Stitches clock and compass frames into the Bedrock atlases when the pack overrides every frame
/// at one size; a partial set would leave holes in the atlas.
pub(crate) fn atlas_textures(items: &HashMap<String, RgbaImage>) -> Vec<(String, RgbaImage)> {
    let mut out = Vec::new();
    'atlases: for (prefix, atlas, count) in ITEM_ATLASES {
        let mut frames = Vec::with_capacity(count);
        for index in 0..count {
            let Some(frame) = items.get(&format!("{prefix}{index:02}")) else {
                continue 'atlases;
            };
            if frames
                .first()
                .is_some_and(|first: &&RgbaImage| first.dimensions() != frame.dimensions())
            {
                continue 'atlases;
            }
            frames.push(frame);
        }
        let (width, height) = frames[0].dimensions();
        let mut canvas = RgbaImage::new(width, height * count as u32);
        for (index, frame) in frames.iter().enumerate() {
            paste(&mut canvas, frame, 0, index as u32 * height);
        }
        out.push((format!("textures/items/{atlas}.png"), canvas));
    }
    out
}
