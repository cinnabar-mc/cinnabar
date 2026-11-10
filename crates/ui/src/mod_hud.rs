//! Bounded cosmetic HUD data rendered by the host's JSON-UI engine.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

mod anchor;
mod bindings;
mod elements;
mod geometry;
pub use bindings::{HudBindings, MAX_HUD_BINDINGS};
pub use elements::{
    ElementState, ElementStates, HiddenElements, HudViewport, PanelLayout, VanillaElement,
};
pub use geometry::{HudGeometry, HudGeometryRequests, MAX_HUD_GEOMETRY_REQUESTS};
/// Bound for one read-only server text field crossing an extension callback.
pub const MAX_OBSERVED_TEXT_BYTES: usize = 256;
/// Bound for the newest received chat history imported by one callback.
pub const MAX_OBSERVED_CHAT_MESSAGES: usize = 32;
/// Maximum received messages returned by one incremental history import.
pub const MAX_CHAT_READ_MESSAGES: usize = 4;
pub const MAX_HUD_CARDS: usize = 64;
pub const MAX_TEXT_BYTES: usize = 96;
/// Uniform HUD scales shared by admission and the native layout editor.
pub const MIN_CARD_SCALE: f32 = 0.25;
pub const MAX_CARD_SCALE: f32 = 5.;
/// Scale span of the editor's continuous `hud.scale` slider, from its left to its right edge.
pub const EDITOR_SCALE_SLIDER_RANGE: [f32; 2] = [0.5, 2.];
const _: () = assert!(
    EDITOR_SCALE_SLIDER_RANGE[0] >= MIN_CARD_SCALE
        && EDITOR_SCALE_SLIDER_RANGE[0] < EDITOR_SCALE_SLIDER_RANGE[1]
        && EDITOR_SCALE_SLIDER_RANGE[1] <= MAX_CARD_SCALE
);
pub const MAX_CARD_WIDTH: f32 = 512.;
/// Largest unscaled corner radius, in GUI pixels, of card and text-plate backgrounds.
pub const MAX_CARD_BACKGROUND_RADIUS: f32 = 16.;
/// Card background radius when omitted, matching the original card template.
pub const DEFAULT_CARD_BACKGROUND_RADIUS: f32 = 3.;
pub const DEFAULT_CARD_WIDTH: f32 = 148.;

/// Accepts a local card name; qualified and foreign-owner names cannot be selected.
pub fn valid_card_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 48
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Anchor {
    #[default]
    TopLeft,
    /// Keeps the card horizontally centered as its size or viewport changes.
    TopCenter,
    TopRight,
    Left,
    Center,
    Right,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hud {
    pub cards: Vec<Card>,
    /// Editor requests persist completed gestures when explicitly enabled.
    #[serde(default)]
    pub autosave: bool,
    /// Enables host-owned corner handles for uniform card resizing.
    #[serde(default)]
    pub resizable: bool,
    /// Host-only inclusive scale bounds narrower than the card range, for host previews whose
    /// painter supports fewer sizes. JSON cannot set this bound.
    #[serde(skip)]
    pub scale_range: Option<[f32; 2]>,
    /// Hides editor labels while preserving native draggable outlines.
    #[serde(default)]
    pub hide_editor_labels: bool,
    /// Optional client-authored JSON-UI chrome for a host-owned layout editor.
    #[serde(default)]
    pub surface: Option<crate::mod_panel::Surface>,
    /// Optional editor settings-card size in GUI pixels, positioned beside the selection.
    #[serde(default)]
    pub inspector_size: Option<[f32; 2]>,
}

/// Selects host-owned typography for a draggable HUD card.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardFont {
    #[default]
    Native,
    Sans,
}

impl CardFont {
    /// Returns the same font alias for painting and native text measurement.
    pub fn name(self) -> &'static str {
        match self {
            Self::Native => "default",
            Self::Sans => crate::mod_panel::SANS_FONT_NAME,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Card {
    pub id: String,
    /// Initial visibility in gameplay and the editor; hidden cards remain available for editing.
    #[serde(default = "enabled")]
    pub visible: bool,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub editor_label: String,
    /// Host-measured preview bounds in GUI pixels. JSON cannot set this geometry.
    #[serde(skip)]
    pub editor_size: Option<[f32; 2]>,
    #[serde(default)]
    pub anchor: Anchor,
    /// GUI pixels inward from edge anchors, or right/down from centered axes.
    #[serde(default)]
    pub offset: [f32; 2],
    #[serde(default = "unit")]
    pub scale: f32,
    /// Fractions of available travel (viewport minus the scaled card size).
    #[serde(default)]
    pub position: Option<[f32; 2]>,
    #[serde(default = "background_opacity")]
    pub background_opacity: f32,
    /// Unscaled GUI-pixel background corner radius, 0 through 16; paint clamps it to half the
    /// shorter side.
    #[serde(default = "background_radius")]
    pub background_radius: f32,
    /// Unscaled GUI-pixel width, 1 through 512.
    #[serde(default = "card_width")]
    pub width: f32,
    /// Omitted typography keeps the native pixel font.
    #[serde(default)]
    pub font: CardFont,
    /// Optional factory placement used only by the host-owned layout editor.
    #[serde(default)]
    pub reset_anchor: Option<Anchor>,
    #[serde(default)]
    pub reset_offset: Option<[f32; 2]>,
    /// Optional factory scale restored by the layout editor.
    #[serde(default)]
    pub reset_scale: Option<f32>,
}

impl Default for Card {
    fn default() -> Self {
        Self {
            id: String::new(),
            visible: true,
            title: String::new(),
            editor_label: String::new(),
            editor_size: None,
            anchor: Anchor::default(),
            offset: [0.; 2],
            scale: unit(),
            position: None,
            background_opacity: background_opacity(),
            background_radius: background_radius(),
            width: card_width(),
            font: CardFont::default(),
            reset_anchor: None,
            reset_offset: None,
            reset_scale: None,
        }
    }
}

impl Card {
    /// Names editor rows and feedback consistently, falling back to the stable card ID.
    pub fn editor_name(&self) -> &str {
        if self.editor_label.is_empty() {
            &self.id
        } else {
            &self.editor_label
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Placement {
    pub id: String,
    pub anchor: Anchor,
    pub offset: [f32; 2],
    pub scale: f32,
    pub position: Option<[f32; 2]>,
}

impl Placement {
    /// Captures authored placement without card artwork, text or runtime layout caches.
    pub fn capture(card: &Card) -> Self {
        Self {
            id: card.id.clone(),
            anchor: card.anchor,
            offset: card.offset,
            scale: card.scale,
            position: card.position,
        }
    }

    /// Restores placement onto a card already matched by ID, leaving its content intact.
    pub fn apply_to(&self, card: &mut Card) {
        card.anchor = self.anchor;
        card.offset = self.offset;
        card.scale = self.scale;
        card.position = self.position;
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EditorResult {
    pub saved: bool,
    pub reset: bool,
    pub placements: Vec<Placement>,
    /// IDs hidden in this saved draft; every ID also has a placement.
    pub hidden_cards: Vec<String>,
    /// A history restore replaces unread reset intent instead of coalescing it.
    pub replace_pending_reset: bool,
}

fn unit() -> f32 {
    1.
}
/// Admits a finite unscaled background corner radius within its published bound.
pub(crate) fn background_radius_valid(radius: f32) -> bool {
    radius.is_finite() && (0. ..=MAX_CARD_BACKGROUND_RADIUS).contains(&radius)
}
/// Keeps a card visible when it omits its visibility.
fn enabled() -> bool {
    true
}
/// Preserves the existing card surface alpha when the new field is omitted.
fn background_opacity() -> f32 {
    0.82
}
/// Keeps the original card rounding when the radius is omitted.
fn background_radius() -> f32 {
    DEFAULT_CARD_BACKGROUND_RADIUS
}
/// Keeps omitted dimensions identical to the original host-owned card template.
fn card_width() -> f32 {
    DEFAULT_CARD_WIDTH
}
/// Whether `value` is the bounded plain single-line text HUD cards and editor captions accept.
pub fn plain_text_valid(value: &str) -> bool {
    text(value).is_ok()
}

fn text(value: &str) -> Result<(), String> {
    if value.len() > MAX_TEXT_BYTES || value.chars().any(|c| c.is_control() || c == '§') {
        return Err("HUD text must be bounded plain single-line text".into());
    }
    Ok(())
}
impl Hud {
    /// Inclusive bounds every card scale and editor scaling gesture stays within.
    pub fn scale_bounds(&self) -> [f32; 2] {
        self.scale_range.unwrap_or([MIN_CARD_SCALE, MAX_CARD_SCALE])
    }

    /// Rejects unbounded or malformed guest data before publication.
    pub fn validate(&self) -> Result<(), String> {
        if self.scale_range.is_some_and(|[lo, hi]| {
            !(lo.is_finite()
                && hi.is_finite()
                && MIN_CARD_SCALE <= lo
                && lo < hi
                && hi <= MAX_CARD_SCALE)
        }) {
            return Err("HUD scale bounds exceed the card scale range".into());
        }
        let [min_scale, max_scale] = self.scale_bounds();
        if self.inspector_size.is_some_and(|size| {
            size.into_iter()
                .any(|value| !value.is_finite() || value <= 0.)
        }) {
            return Err("HUD inspector size must contain finite positive GUI pixels".into());
        }
        if let Some(surface) = &self.surface {
            if !surface.collections.is_empty() {
                return Err("HUD editor surfaces do not support tile collections".into());
            }
            surface.validate()?;
            for action in surface.actions()? {
                let indexed = |prefix: &str, maximum: usize| {
                    action
                        .strip_prefix(prefix)
                        .and_then(|index| index.parse::<usize>().ok())
                        .is_some_and(|index| index < maximum)
                };
                if !matches!(
                    action.as_str(),
                    "hud.save"
                        | "hud.cancel"
                        | "hud.close"
                        | "hud.reset"
                        | "hud.grid"
                        | "hud.snap"
                        | "hud.undo"
                        | "hud.redo"
                        | "hud.scale"
                        | "hud.scale_down"
                        | "hud.scale_up"
                ) && !indexed("hud.card:", MAX_HUD_CARDS)
                    && !indexed("hud.select:", MAX_HUD_CARDS)
                    && !indexed("hud.visibility:", MAX_HUD_CARDS)
                    && !indexed("hud.anchor:", Anchor::ALL.len())
                    && !indexed("hud.control:", crate::mod_panel::MAX_PANEL_CONTROLS)
                    && !indexed("hud.done:", crate::mod_panel::MAX_PANEL_CONTROLS)
                {
                    return Err("HUD editor surface action is unsupported".into());
                }
            }
        }
        if self.cards.len() > MAX_HUD_CARDS {
            return Err("HUD has too many cards".into());
        }
        let mut ids = HashSet::new();
        for card in &self.cards {
            if !valid_card_id(&card.id) || !ids.insert(&card.id) {
                return Err("HUD cards require bounded unique identifiers".into());
            }
            text(&card.title)?;
            text(&card.editor_label)?;
            if card
                .editor_size
                .is_some_and(|size| size.into_iter().any(|side| !side.is_finite() || side <= 0.))
                || card.reset_scale.is_some_and(|scale| {
                    !scale.is_finite() || !(min_scale..=max_scale).contains(&scale)
                })
                || !card.scale.is_finite()
                || !(min_scale..=max_scale).contains(&card.scale)
                || card.offset.into_iter().any(|v| !v.is_finite())
                || !card.background_opacity.is_finite()
                || !(0. ..=1.).contains(&card.background_opacity)
                || !background_radius_valid(card.background_radius)
                || !card.width.is_finite()
                || !(1. ..=MAX_CARD_WIDTH).contains(&card.width)
                || card.position.is_some_and(|p| {
                    p.into_iter()
                        .any(|v| !v.is_finite() || !(0. ..=1.).contains(&v))
                })
                || card
                    .reset_offset
                    .is_some_and(|p| p.into_iter().any(|v| !v.is_finite()))
            {
                return Err("HUD card geometry exceeds its bounds".into());
            }
        }
        Ok(())
    }
}
impl EditorResult {
    /// Rejects malformed placement output and changes attached to cancellation.
    pub fn validate(&self) -> Result<(), String> {
        if !self.saved
            && (self.reset
                || !self.placements.is_empty()
                || !self.hidden_cards.is_empty()
                || self.replace_pending_reset)
        {
            return Err("cancelled HUD editor cannot carry changes".into());
        }
        let hud = Hud {
            cards: self
                .placements
                .iter()
                .map(|p| Card {
                    id: p.id.clone(),
                    anchor: p.anchor,
                    offset: p.offset,
                    position: p.position,
                    scale: p.scale,
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        hud.validate()?;
        let mut hidden = HashSet::new();
        if self.hidden_cards.iter().any(|id| {
            !hidden.insert(id) || !self.placements.iter().any(|placement| &placement.id == id)
        }) {
            return Err("hidden HUD cards require unique placement IDs".into());
        }
        Ok(())
    }
}
#[cfg(test)]
#[path = "mod_hud/tests.rs"]
mod test_suite;
