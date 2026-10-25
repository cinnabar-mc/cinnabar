//! Bounded, host-rendered controls for a granted personal extension.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

pub use surface::{definitions, hud_control, local_player_head};

mod axis;
mod control;
pub use control::{
    Control, Event, FIELD_NAVIGATION_KEYS, FieldEvent, FieldKey, FieldModifiers,
    MAX_PANEL_VALUE_CHARS, TextEvent, is_field_navigation_key, validate_text_value,
};
pub(crate) mod surface;
mod update;
pub use axis::{AxisExtent, AxisRoute};
pub use surface::{
    ActorPoint, CollectionArt, CollectionDefinition, CollectionItem, HudIndex, ItemValue,
    MAX_HUD_SURFACE_BINDINGS, MAX_SURFACE_BINDINGS, MAX_SURFACE_NODES, MAX_SURFACE_PATCH_BYTES,
    MAX_TRANSITION_SECONDS, PROJECTED_VISIBLE_SUFFIX, ProjectValue, Projection, Surface,
    SurfaceCollection, SurfaceItem, SurfacePatch, SurfaceRole, SurfaceTransition, SurfaceValue,
    TransitionEasing, WorldPoint, backdrop, projected_visible_name,
};
pub use update::{
    CollectionReveal, CollectionRevealRequest, MAX_PANEL_BUNDLE_BYTES, MAX_PANEL_PATCH_BYTES,
    MAX_PANEL_SURFACES, PanelBundle, PanelPatch,
};

/// A whole panel's JSON: its surface document escaped once, bindings and controls.
pub const MAX_PANEL_BYTES: usize = 320 * 1024;
pub const MAX_PANEL_CONTROLS: usize = 512;
/// Input batches remain bounded independently of the retained control catalog.
pub const MAX_PANEL_EVENTS: usize = 64;
pub const MAX_PANEL_TEXT_BYTES: usize = 96;
pub const MAX_PANEL_ID_BYTES: usize = 48;
pub const MAX_PANEL_CHOICES: usize = 8;
pub const FONT_NAME: &str = "mod_panel";
/// Glyph records in one bounded private atlas, including text, punctuation and icons.
pub const MAX_FONT_GLYPHS: usize = 512;
/// A shipped, smoothly sampled sans face for authored native panels.
pub const SANS_FONT_NAME: &str = "mod_panel_sans";
/// Fonts a surface label selects through `font_type`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LabelFont {
    /// The game's own face, `font_type` `default`.
    #[default]
    Native,
    /// The shipped smooth face, [`SANS_FONT_NAME`].
    Sans,
}

impl LabelFont {
    /// The `font_type` a surface label names this face with.
    pub fn name(self) -> &'static str {
        match self {
            Self::Native => "default",
            Self::Sans => SANS_FONT_NAME,
        }
    }
}

/// Opens the selected personal panel when its authored hotkey is unavailable on a keyboard.
pub const FALLBACK_TOGGLE_KEY: &str = "F10";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Panel {
    /// The extension-authored JSON-UI the panel draws.
    pub surface: Surface,
    /// Logical authored-surface dimensions to fit within smaller viewports.
    #[serde(default)]
    pub reference_size: Option<[f32; 2]>,
    pub toggle_key: String,
    /// Value-bearing input widgets the host routes surface actions to.
    #[serde(default)]
    pub controls: Vec<Control>,
    /// Colours of the host-drawn popup widgets; omitted ones use the built-in palette.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub widget_colors: Option<WidgetColors>,
    /// Lets a binding control receive Escape instead of dismissing the panel.
    #[serde(default)]
    pub capture_key: bool,
    /// Keeps the resource-pack gameplay HUD visible behind this panel.
    /// Hide HUD and death screens still apply; focused panels always hide the crosshair.
    #[serde(default)]
    pub show_gameplay_hud: bool,
    /// Enables native feedback easing, smooth wheel scrolling and surface transitions.
    #[serde(default)]
    pub motion: bool,
}

/// Straight RGBA colours of the host-drawn popup widgets: the slider value editor, the choice
/// dropdown and keybind capture. Each omitted colour keeps its built-in value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WidgetColors {
    /// Behind the whole popup.
    pub background: Option<[f32; 4]>,
    /// Fields and rows inside the popup.
    pub surface: Option<[f32; 4]>,
    pub text: Option<[f32; 4]>,
    /// The caret, selection and highlighted row.
    pub accent: Option<[f32; 4]>,
    pub border: Option<[f32; 4]>,
}

impl WidgetColors {
    /// Requires finite channels from 0 to 1.
    pub fn validate(&self) -> Result<(), String> {
        let Self {
            background,
            surface,
            text,
            accent,
            border,
        } = self;
        if [background, surface, text, accent, border]
            .into_iter()
            .flatten()
            .flatten()
            .all(|channel| channel.is_finite() && (0.0..=1.0).contains(channel))
        {
            Ok(())
        } else {
            Err("panel widget colors must be RGBA channels from 0 to 1".into())
        }
    }
}

impl Panel {
    /// Accepts the authored binding or the native keyboard-independent fallback.
    pub fn is_toggle_key(&self, key: &str) -> bool {
        key == self.toggle_key || key == FALLBACK_TOGGLE_KEY
    }
    /// Validates before retaining any guest-controlled text or input geometry.
    pub fn validate(&self) -> Result<(), String> {
        self.validate_inner(true)
    }

    /// Checks changing values after the caller proves the schema and document are unchanged.
    pub fn validate_values(&self) -> Result<(), String> {
        self.validate_inner(false)
    }

    /// Keeps static action checks separate from bounded value-only updates.
    fn validate_inner(&self, document: bool) -> Result<(), String> {
        if let Some(size) = self.reference_size
            && !size
                .iter()
                .all(|value| value.is_finite() && *value > 0.0 && *value <= 1_000_000.0)
        {
            return Err("panel reference size requires bounded positive dimensions".into());
        }
        if let Some(colors) = &self.widget_colors {
            colors.validate()?;
        }
        let surface = &self.surface;
        if document {
            surface.validate()?;
        } else {
            surface.validate_bindings()?;
        }
        for action in if document {
            surface.actions()?
        } else {
            Vec::new()
        } {
            if action == "mod.close" {
                continue;
            }
            let valid = if let Some(index) = action.strip_prefix("mod.control:") {
                index
                    .parse::<usize>()
                    .ok()
                    .is_some_and(|index| index < self.controls.len())
            } else if let Some(axis) = AxisRoute::parse(&action) {
                matches!(
                    self.controls.get(axis.control),
                    Some(Control::Slider { .. })
                ) && axis
                    .extent
                    .is_none_or(|extent| extent.resolve(&surface.bindings).is_some())
            } else if let Some(indices) = action
                .strip_prefix("mod.vector:")
                .or_else(|| action.strip_prefix("mod.drag:"))
            {
                indices.split_once(':').is_some_and(|(x, y)| {
                    let index = |s: &str| {
                        s.parse::<usize>().ok().filter(|i| {
                            matches!(self.controls.get(*i), Some(Control::Slider { .. }))
                        })
                    };
                    match (index(x), index(y)) {
                        (Some(x), Some(y)) => x != y,
                        _ => false,
                    }
                })
            } else if let Some(index) = action.strip_prefix("mod.edit:") {
                index.parse::<usize>().ok().is_some_and(|index| {
                    matches!(
                        self.controls.get(index),
                        Some(Control::Slider { .. } | Control::Text { .. })
                    )
                })
            } else {
                false
            };
            if !valid {
                return Err("panel surface action must reference a declared control".into());
            }
        }
        if self.toggle_key.is_empty()
            || self.toggle_key.len() > MAX_PANEL_ID_BYTES
            || !self
                .toggle_key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric())
        {
            return Err("panel toggle key must be a bounded physical key name".into());
        }
        if self.controls.len() > MAX_PANEL_CONTROLS {
            return Err("panel has too many controls".into());
        }
        let mut ids = HashSet::with_capacity(self.controls.len());
        for control in &self.controls {
            let id = control.id();
            if id.is_empty()
                || id.len() > MAX_PANEL_ID_BYTES
                || !id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"_-.".contains(&byte))
                || !ids.insert(id)
            {
                return Err("panel control IDs must be bounded, unique identifiers".into());
            }
            text(control.label(), "control label")?;
            match control {
                Control::Text {
                    value,
                    min_chars,
                    max_chars,
                    ..
                } => validate_text_value(value, *min_chars, *max_chars)?,
                Control::Keybind { key, .. } => {
                    if key.len() > MAX_PANEL_ID_BYTES || key.chars().any(char::is_control) {
                        return Err("panel keybind requires a bounded key label".into());
                    }
                }
                Control::Slider {
                    value,
                    min,
                    max,
                    step,
                    ..
                } => {
                    if ![value, min, max, step]
                        .iter()
                        .all(|number| number.is_finite())
                        || min >= max
                        || value < min
                        || value > max
                        || *step <= 0.0
                        || step > &(max - min)
                        || !(max - min).is_finite()
                        || ((max - min) / step) > 1_000_000.0
                    {
                        return Err(
                            "panel slider requires finite, ordered bounds and a positive step"
                                .into(),
                        );
                    }
                }
                Control::Choice { index, options, .. } => {
                    if options.is_empty()
                        || options.len() > MAX_PANEL_CHOICES
                        || *index as usize >= options.len()
                    {
                        return Err(
                            "panel choice requires bounded options and a valid index".into()
                        );
                    }
                    for option in options {
                        text(option, "choice label")?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
}

fn text(value: &str, field: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > MAX_PANEL_TEXT_BYTES || value.chars().any(char::is_control)
    {
        Err(format!("panel {field} must be bounded, single-line text"))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
