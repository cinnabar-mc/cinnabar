//! Saved touch-control placements: positions as fractions of available safe-screen travel, plus
//! optional uniform size multipliers.
use super::SettingsOptions;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use ui::mod_touch::TOUCH_CONTROL_SCALE;

/// Fixed-point resolution of persisted normalized positions.
const UNITS: f32 = 10_000.;
/// Fixed-point resolution of persisted scales.
const SCALE_UNITS: f32 = 1_000.;
/// Mod button placements kept for mods that are not loaded, beyond which later names drop.
const MAX_MOD_PLACEMENTS: usize = 64;

/// Only explicit placements are stored; an empty map retains the authored touch layout.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TouchLayout(BTreeMap<String, Placement>);

/// One control's fixed-point position and, when resized, its fixed-point scale.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(from = "Stored", into = "Stored")]
struct Placement {
    position: [u16; 2],
    scale: Option<u16>,
}

/// Persisted form: `[x, y]` keeps the authored size and `[x, y, scale]` resizes the control.
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum Stored {
    Scaled([u16; 3]),
    Placed([u16; 2]),
}

impl From<Stored> for Placement {
    fn from(stored: Stored) -> Self {
        match stored {
            Stored::Scaled([x, y, scale]) => Self {
                position: [x, y],
                scale: Some(scale),
            },
            Stored::Placed(position) => Self {
                position,
                scale: None,
            },
        }
    }
}

impl From<Placement> for Stored {
    fn from(placement: Placement) -> Self {
        let [x, y] = placement.position;
        match placement.scale {
            Some(scale) => Self::Scaled([x, y, scale]),
            None => Self::Placed([x, y]),
        }
    }
}

/// Whether a fixed-point scale lies within the player-selectable touch control range.
fn valid_scale(scale: u16) -> bool {
    let [lo, hi] = TOUCH_CONTROL_SCALE.map(|bound| (bound * SCALE_UNITS).round() as u16);
    (lo..=hi).contains(&scale)
}

impl TouchLayout {
    /// Reads a validated position as a fraction of travel after subtracting control size.
    pub fn position(&self, name: &str) -> Option<[f32; 2]> {
        self.0
            .get(name)
            .map(|placement| placement.position.map(|v| f32::from(v) / UNITS))
    }
    /// The control's uniform size multiplier; unplaced and unresized controls keep `1`.
    pub fn scale(&self, name: &str) -> f32 {
        self.0
            .get(name)
            .and_then(|placement| placement.scale)
            .map_or(1., |scale| f32::from(scale) / SCALE_UNITS)
    }
    /// Stores a finite, bounded position at the authored size.
    pub fn set(&mut self, name: &str, position: [f32; 2]) {
        self.set_scaled(name, position, 1.);
    }
    /// Stores a finite, bounded position and a scale within [`TOUCH_CONTROL_SCALE`]; the
    /// scale's travel fraction subtracts the scaled control size. Control identities are
    /// checked when loading settings.
    pub fn set_scaled(&mut self, name: &str, position: [f32; 2], scale: f32) {
        let scale = (scale * SCALE_UNITS).round();
        let scale = (scale != SCALE_UNITS).then_some(scale as u16);
        if position
            .into_iter()
            .all(|v| v.is_finite() && (0.0..=1.0).contains(&v))
            && scale.is_none_or(valid_scale)
        {
            self.0.insert(
                name.to_owned(),
                Placement {
                    position: position.map(|v| (v * UNITS).round() as u16),
                    scale,
                },
            );
        }
    }
    /// Keeps only the placements whose names satisfy `keep`.
    pub fn retain(&mut self, mut keep: impl FnMut(&str) -> bool) {
        self.0.retain(|name, _| keep(name));
    }
    /// Removes obsolete control identities and malformed fixed-point coordinates, and resets
    /// out-of-range scales to the authored size. Mod button placements are kept by name,
    /// bounded, whether or not their mod is loaded.
    pub(super) fn validate(&mut self) {
        if let Some(position) = self.0.remove("emote") {
            self.0.entry("utilities".into()).or_insert(position);
        }
        let document: serde_json::Value =
            serde_json::from_slice(assets::TOUCH_LAYOUT).expect("built-in touch layout parses");
        let mut names = std::collections::BTreeSet::new();
        control_names(&document, &mut names);
        let mut mods = 0;
        self.0.retain(|name, placement| {
            let known = if ui::mod_touch::is_placement_name(name) {
                mods += 1;
                mods <= MAX_MOD_PLACEMENTS
            } else {
                name != "look" && names.contains(name.as_str())
            };
            placement.scale = placement
                .scale
                .filter(|&scale| valid_scale(scale) && f32::from(scale) != SCALE_UNITS);
            known && placement.position.iter().all(|v| f32::from(*v) <= UNITS)
        });
    }
}
/// Derives identities from the authored control actions rather than maintaining a second registry.
fn control_names<'a>(
    value: &'a serde_json::Value,
    names: &mut std::collections::BTreeSet<&'a str>,
) {
    match value {
        serde_json::Value::String(text) => {
            if let Some(name) = text
                .strip_prefix("cinnabar.touch.")
                .filter(|name| !name.is_empty())
            {
                names.insert(name);
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                control_names(value, names);
            }
        }
        serde_json::Value::Object(values) => {
            for value in values.values() {
                control_names(value, names);
            }
        }
        _ => {}
    }
}

impl SettingsOptions {
    /// The saved layout used by both touch rendering and input regions.
    pub fn touch_layout(&self) -> &TouchLayout {
        &self.touch_layout
    }
    /// Replaces a completed layout draft; callers persist only after Save.
    pub fn set_touch_layout(&mut self, mut layout: TouchLayout) {
        layout.validate();
        self.touch_layout = layout;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn emote_button_placement_migrates_to_the_utility_trigger() {
        let options =
            SettingsOptions::decode(br#"{"touch_layout":{"emote":[1234,7500]}}"#).unwrap();
        assert_eq!(
            options.touch_layout().position("utilities"),
            Some([0.1234, 0.75])
        );
        assert_eq!(options.touch_layout().position("emote"), None);
        let both = SettingsOptions::decode(
            br#"{"touch_layout":{"emote":[1234,7500],"utilities":[2500,5000]}}"#,
        )
        .unwrap();
        assert_eq!(both.touch_layout().position("utilities"), Some([0.25, 0.5]));
    }

    #[test]
    fn positions_round_trip_and_reject_stale_or_invalid_controls() {
        let mut options = SettingsOptions::default();
        let mut layout = TouchLayout::default();
        layout.set("jump", [0.1234, 0.75]);
        layout.set("look", [0.5, 0.5]);
        layout.set("old-control", [0.5, 0.5]);
        layout.set("chat", [f32::NAN, 0.]);
        options.set_touch_layout(layout);
        let decoded = SettingsOptions::decode(&serde_json::to_vec(&options).unwrap()).unwrap();
        assert_eq!(
            decoded.touch_layout().position("jump"),
            Some([0.1234, 0.75])
        );
        for name in ["look", "old-control", "chat"] {
            assert_eq!(decoded.touch_layout().position(name), None);
        }
        let old = SettingsOptions::decode(b"{}").unwrap();
        assert_eq!(old.touch_layout(), &TouchLayout::default());
        let corrupt = SettingsOptions::decode(br#"{"touch_layout":{"jump":[65535,4]}}"#).unwrap();
        assert_eq!(corrupt.touch_layout().position("jump"), None);
    }

    /// Resized controls round-trip their scale, while older two-value placements, unit scales
    /// and out-of-range scales keep the authored size.
    #[test]
    fn scales_round_trip_beside_legacy_positions() {
        let mut layout = TouchLayout::default();
        layout.set_scaled("jump", [0.25, 0.5], 1.75);
        layout.set_scaled("sneak", [0.25, 0.5], 1.);
        layout.set_scaled("sprint", [0.25, 0.5], 100.);
        let mut options = SettingsOptions::default();
        options.set_touch_layout(layout);
        let encoded = serde_json::to_value(&options).unwrap();
        assert_eq!(
            encoded["touch_layout"]["jump"],
            serde_json::json!([2500, 5000, 1750])
        );
        assert_eq!(
            encoded["touch_layout"]["sneak"],
            serde_json::json!([2500, 5000])
        );
        let decoded = SettingsOptions::decode(&serde_json::to_vec(&options).unwrap()).unwrap();
        let layout = decoded.touch_layout();
        assert_eq!(layout.position("jump"), Some([0.25, 0.5]));
        assert_eq!(layout.scale("jump"), 1.75);
        assert_eq!(layout.scale("sneak"), 1.);
        assert_eq!(layout.position("sprint"), None);
        let stored = SettingsOptions::decode(
            br#"{"touch_layout":{"jump":[2500,5000],"chat":[1,2,9000],"menu":[1,2,1000]}}"#,
        )
        .unwrap();
        let layout = stored.touch_layout();
        assert_eq!(layout.scale("jump"), 1.);
        assert_eq!(layout.position("chat"), Some([0.0001, 0.0002]));
        assert_eq!(layout.scale("chat"), 1.);
        assert_eq!(layout.scale("menu"), 1.);
        assert_eq!(
            serde_json::to_value(layout).unwrap()["menu"],
            serde_json::json!([1, 2])
        );
    }

    /// Mod button placements persist by id while their mod is unloaded, within a bound.
    #[test]
    fn mod_button_placements_persist_by_name_and_are_bounded() {
        let mut layout = TouchLayout::default();
        layout.set("mod-client-menu", [0.25, 0.5]);
        layout.set("mod-Bad", [0.25, 0.5]);
        for index in 0..MAX_MOD_PLACEMENTS {
            layout.set(&format!("mod-x{index:03}"), [0.5, 0.5]);
        }
        let mut options = SettingsOptions::default();
        options.set_touch_layout(layout);
        let decoded = SettingsOptions::decode(&serde_json::to_vec(&options).unwrap()).unwrap();
        let layout = decoded.touch_layout();
        assert_eq!(layout.position("mod-client-menu"), Some([0.25, 0.5]));
        assert_eq!(layout.position("mod-Bad"), None);
        assert_eq!(layout.position("mod-x000"), Some([0.5, 0.5]));
        assert_eq!(
            layout.position(&format!("mod-x{:03}", MAX_MOD_PLACEMENTS - 1)),
            None
        );
    }
}
