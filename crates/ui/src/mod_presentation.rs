//! Parameters mods set on native world presentation, and their deterministic merge.
//!
//! Each mod retains one [`ModPresentation`]. The client merges every active mod's output in load
//! order into one [`Presentation`]: the earliest mod that sets a single policy (nametag style,
//! hurt overlay, chat actions) owns it; per-actor rows are owned per runtime ID and surface by the
//! earliest mod whose grants admit that actor's kind; tooltip lines are appended in load order.

use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

/// Most annotated actors one mod retains.
pub const MAX_ANNOTATIONS: usize = 128;
/// Most text spans in one nametag or player-list annotation.
pub const MAX_ANNOTATION_SPANS: usize = 4;
/// Most UTF-8 bytes in one annotation span.
pub const MAX_ANNOTATION_SPAN_BYTES: usize = 32;
/// Most tinted actors one mod retains.
pub const MAX_ACTOR_TINTS: usize = 64;
/// Most tooltip lines one mod retains, across every item.
pub const MAX_TOOLTIP_LINES: usize = 256;
/// Most mod lines appended to one item's tooltip, across every mod.
pub const MAX_ITEM_TOOLTIP_LINES: usize = 8;
/// Most UTF-8 bytes in one tooltip line.
pub const MAX_TOOLTIP_LINE_BYTES: usize = 96;
/// Most UTF-8 bytes in one tooltip item identifier.
pub const MAX_TOOLTIP_IDENTIFIER_BYTES: usize = 128;
/// Most clickable chat spans.
pub const MAX_CHAT_ACTIONS: usize = 32;
/// Most UTF-8 bytes of chat text one action matches.
pub const MAX_CHAT_ACTION_TEXT_BYTES: usize = 96;

/// Colour channels in 0–1, with alpha last.
pub type Rgba = [f32; 4];

/// Rejects non-finite or out-of-range colour channels.
pub fn validate_color(color: Rgba) -> Result<(), String> {
    if color
        .into_iter()
        .any(|v| !v.is_finite() || !(0. ..=1.).contains(&v))
    {
        return Err("presentation colours must contain finite values between zero and one".into());
    }
    Ok(())
}

/// Accepts nonempty single-line text within `max` bytes, without style codes.
fn plain_text(text: &str, max: usize) -> bool {
    !text.is_empty() && text.len() <= max && !text.chars().any(|c| c.is_control() || c == '§')
}

/// Parameters of the native nametag renderer.
#[derive(Clone, Debug, PartialEq)]
pub struct NametagStyle {
    pub hide: bool,
    pub hide_players: bool,
    pub hide_passengers: bool,
    pub show_self: bool,
    pub show_when_hud_hidden: bool,
    /// Multiplies every tag's alpha, 0 through 1.
    pub opacity: f32,
    /// Uniform tag scale, 0.5 through 3.
    pub scale: f32,
    pub shadow: bool,
    /// Border width in tag pixels, 0 through 8, drawn only with a border colour.
    pub border_width: f32,
    /// Replaces the native text colour of other actors' tags, and of the local tag without
    /// `own_text_color`; annotation spans keep their own colour.
    pub text_color: Option<Rgba>,
    pub own_text_color: Option<Rgba>,
    pub border_color: Option<Rgba>,
    pub background_color: Option<Rgba>,
}

impl Default for NametagStyle {
    /// Native nametags, with a half-pixel border width for when a border colour is set.
    fn default() -> Self {
        Self {
            hide: false,
            hide_players: false,
            hide_passengers: false,
            show_self: false,
            show_when_hud_hidden: false,
            opacity: 1.,
            scale: 1.,
            shadow: false,
            border_width: 0.5,
            text_color: None,
            own_text_color: None,
            border_color: None,
            background_color: None,
        }
    }
}

impl NametagStyle {
    /// Bounds opacity, scale, border width and every colour.
    pub fn validate(&self) -> Result<(), String> {
        if !(0. ..=1.).contains(&self.opacity)
            || !(0.5..=3.).contains(&self.scale)
            || !(0. ..=8.).contains(&self.border_width)
        {
            return Err("nametag opacity, scale or border width is out of bounds".into());
        }
        [
            self.text_color,
            self.own_text_color,
            self.border_color,
            self.background_color,
        ]
        .into_iter()
        .flatten()
        .try_for_each(validate_color)
    }
}

/// Where annotation spans go relative to the native name. Spans on the name's line are separated
/// from it by one space; a player-list row has one line, so roster spans placed above or below
/// follow the name.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SpanPosition {
    /// On the name's first line, before it.
    Before,
    /// On the name's first line, after it.
    #[default]
    After,
    /// On its own line above the name.
    Above,
    /// On its own line below the name.
    Below,
}

/// One coloured run of plain text.
#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    pub text: Arc<str>,
    pub color: Rgba,
}

/// The spans one surface draws beside a native name.
#[derive(Clone, Debug, PartialEq)]
pub struct Spans {
    pub spans: Box<[Span]>,
    pub position: SpanPosition,
}

impl Spans {
    /// Requires 1 through [`MAX_ANNOTATION_SPANS`] bounded plain-text spans with valid colours.
    pub fn validate(&self) -> Result<(), String> {
        if self.spans.is_empty() || self.spans.len() > MAX_ANNOTATION_SPANS {
            return Err("annotation span count is out of bounds".into());
        }
        for span in &self.spans {
            if !plain_text(&span.text, MAX_ANNOTATION_SPAN_BYTES) {
                return Err("annotation spans must be short plain text".into());
            }
            validate_color(span.color)?;
        }
        Ok(())
    }

    /// The text of every span, in order.
    pub fn texts(&self) -> impl Iterator<Item = &Arc<str>> {
        self.spans.iter().map(|span| &span.text)
    }
}

/// One actor's annotation; an absent surface draws nothing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Annotation {
    pub nametag: Option<Arc<Spans>>,
    /// Applies only to players, on the player-list row of that player.
    pub roster: Option<Arc<Spans>>,
}

/// The native damage overlay colours; an absent channel keeps native.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HurtOverlay {
    pub body: Option<Rgba>,
    pub armor: Option<Rgba>,
}

/// One extra line appended to the tooltip of matching items.
#[derive(Clone, Debug, PartialEq)]
pub struct TooltipLine {
    pub identifier: String,
    /// None matches every metadata value.
    pub metadata: Option<u32>,
    pub text: String,
    pub color: Rgba,
}

impl TooltipLine {
    /// Requires a bounded identifier and bounded plain text with a valid colour.
    pub fn validate(&self) -> Result<(), String> {
        if self.identifier.is_empty()
            || self.identifier.len() > MAX_TOOLTIP_IDENTIFIER_BYTES
            || self.identifier.chars().any(char::is_control)
            || !plain_text(&self.text, MAX_TOOLTIP_LINE_BYTES)
        {
            return Err("invalid tooltip line".into());
        }
        validate_color(self.color)
    }

    /// Whether this line belongs on the tooltip of `identifier` with `metadata`.
    pub fn matches(&self, identifier: &str, metadata: u32) -> bool {
        self.identifier == identifier && self.metadata.is_none_or(|value| value == metadata)
    }
}

/// Exact plain-text chat span and the event id a primary click on it reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatAction {
    pub text: String,
    pub action: String,
}

impl ChatAction {
    /// Requires bounded single-line text and a short `[A-Za-z0-9_.-]` action id.
    pub fn validate(&self) -> Result<(), String> {
        if self.text.is_empty()
            || self.text.len() > MAX_CHAT_ACTION_TEXT_BYTES
            || self.text.chars().any(char::is_control)
            || self.action.is_empty()
            || self.action.len() > crate::mod_panel::MAX_PANEL_ID_BYTES
            || !self
                .action
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
        {
            return Err("invalid chat action".into());
        }
        Ok(())
    }
}

/// One mod's retained presentation parameters.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModPresentation {
    pub nametag_style: Option<NametagStyle>,
    pub annotations: BTreeMap<u64, Annotation>,
    pub hurt_overlay: HurtOverlay,
    pub tints: BTreeMap<u64, Rgba>,
    pub tooltip_lines: Vec<TooltipLine>,
    pub chat_actions: Vec<ChatAction>,
}

impl ModPresentation {
    /// Whether this mod changes nothing about native presentation.
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

/// The actor kinds a mod's grants admit for per-actor rows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ActorGrants {
    pub players: bool,
    pub entities: bool,
}

/// Separate winners for players and for other actors sharing one runtime ID key.
#[derive(Clone, Debug, PartialEq)]
pub struct ByKind<T> {
    pub player: Option<T>,
    pub other: Option<T>,
}

impl<T> Default for ByKind<T> {
    /// No entry for either kind.
    fn default() -> Self {
        Self {
            player: None,
            other: None,
        }
    }
}

impl<T> ByKind<T> {
    /// Whether neither kind has an entry.
    fn is_empty(&self) -> bool {
        self.player.is_none() && self.other.is_none()
    }

    /// The entry that applies to an actor of this kind.
    pub fn get(&self, player: bool) -> Option<&T> {
        if player {
            self.player.as_ref()
        } else {
            self.other.as_ref()
        }
    }

    /// Fills each still-empty kind the grants admit, so the earliest admitted mod keeps it.
    fn offer(&mut self, grants: ActorGrants, value: &T)
    where
        T: Clone,
    {
        if grants.players && self.player.is_none() {
            self.player = Some(value.clone());
        }
        if grants.entities && self.other.is_none() {
            self.other = Some(value.clone());
        }
    }
}

/// Every active mod's presentation, merged in load order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Presentation {
    pub nametag_style: Option<NametagStyle>,
    pub nametags: HashMap<u64, ByKind<Arc<Spans>>>,
    /// Player-list spans by player runtime ID; only mods with the players grant contribute.
    pub roster: HashMap<u64, Arc<Spans>>,
    pub hurt_overlay: HurtOverlay,
    pub tints: HashMap<u64, ByKind<Rgba>>,
    pub tooltip_lines: Vec<TooltipLine>,
    pub chat_actions: Vec<ChatAction>,
}

impl Presentation {
    /// Merges mods in load order with the actor kinds each one's grants admit.
    pub fn merge<'a>(mods: impl IntoIterator<Item = (ActorGrants, &'a ModPresentation)>) -> Self {
        let mut merged = Self::default();
        let mut hurt_owned = false;
        for (grants, output) in mods {
            if merged.nametag_style.is_none() {
                merged.nametag_style.clone_from(&output.nametag_style);
            }
            if !hurt_owned && output.hurt_overlay != HurtOverlay::default() {
                merged.hurt_overlay = output.hurt_overlay;
                hurt_owned = true;
            }
            for (&runtime_id, annotation) in &output.annotations {
                if let Some(spans) = &annotation.nametag {
                    merged
                        .nametags
                        .entry(runtime_id)
                        .or_default()
                        .offer(grants, spans);
                }
                if let Some(spans) = annotation.roster.as_ref().filter(|_| grants.players) {
                    merged
                        .roster
                        .entry(runtime_id)
                        .or_insert_with(|| Arc::clone(spans));
                }
            }
            merged.nametags.retain(|_, entry| !entry.is_empty());
            for (&runtime_id, color) in &output.tints {
                merged
                    .tints
                    .entry(runtime_id)
                    .or_default()
                    .offer(grants, color);
            }
            merged.tints.retain(|_, entry| !entry.is_empty());
            merged
                .tooltip_lines
                .extend(output.tooltip_lines.iter().cloned());
            if merged.chat_actions.is_empty() {
                merged.chat_actions.clone_from(&output.chat_actions);
            }
        }
        merged
    }

    /// The nametag spans for one actor of this kind.
    pub fn nametag(&self, runtime_id: u64, player: bool) -> Option<&Arc<Spans>> {
        self.nametags.get(&runtime_id)?.get(player)
    }

    /// The overlay tint for one actor of this kind.
    pub fn tint(&self, runtime_id: u64, player: bool) -> Option<Rgba> {
        self.tints.get(&runtime_id)?.get(player).copied()
    }

    /// The first [`MAX_ITEM_TOOLTIP_LINES`] lines for one item, in load order.
    pub fn tooltip_lines(
        &self,
        identifier: &str,
        metadata: u32,
    ) -> impl Iterator<Item = &TooltipLine> {
        self.tooltip_lines
            .iter()
            .filter(move |line| line.matches(identifier, metadata))
            .take(MAX_ITEM_TOOLTIP_LINES)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds validated single-span text for one surface.
    fn spans(text: &str, position: SpanPosition) -> Arc<Spans> {
        Arc::new(Spans {
            spans: [Span {
                text: text.into(),
                color: [1.; 4],
            }]
            .into(),
            position,
        })
    }

    const PLAYERS: ActorGrants = ActorGrants {
        players: true,
        entities: false,
    };
    const ENTITIES: ActorGrants = ActorGrants {
        players: false,
        entities: true,
    };

    /// The earliest mod owns single policies and keyed rows; tooltip lines append in order.
    #[test]
    fn earliest_admitted_mod_owns_each_policy_and_row() {
        let first = ModPresentation {
            nametag_style: Some(NametagStyle {
                scale: 2.,
                ..Default::default()
            }),
            annotations: BTreeMap::from([(
                7,
                Annotation {
                    nametag: Some(spans("first", SpanPosition::After)),
                    roster: None,
                },
            )]),
            tints: BTreeMap::from([(7, [1., 0., 0., 1.])]),
            tooltip_lines: vec![TooltipLine {
                identifier: "minecraft:apple".into(),
                metadata: None,
                text: "first".into(),
                color: [1.; 4],
            }],
            ..Default::default()
        };
        let second = ModPresentation {
            nametag_style: Some(NametagStyle::default()),
            annotations: BTreeMap::from([(
                7,
                Annotation {
                    nametag: Some(spans("second", SpanPosition::Before)),
                    roster: Some(spans("tab", SpanPosition::Above)),
                },
            )]),
            hurt_overlay: HurtOverlay {
                body: Some([0., 0., 1., 1.]),
                armor: None,
            },
            tints: BTreeMap::from([(7, [0., 1., 0., 1.])]),
            tooltip_lines: vec![TooltipLine {
                identifier: "minecraft:apple".into(),
                metadata: Some(1),
                text: "second".into(),
                color: [1.; 4],
            }],
            chat_actions: vec![ChatAction {
                text: "Help".into(),
                action: "help".into(),
            }],
        };
        let both = ActorGrants {
            players: true,
            entities: true,
        };
        let merged = Presentation::merge([(PLAYERS, &first), (both, &second)]);
        assert_eq!(merged.nametag_style.as_ref().unwrap().scale, 2.);
        assert_eq!(&*merged.nametag(7, true).unwrap().spans[0].text, "first");
        assert_eq!(
            &*merged.nametag(7, false).unwrap().spans[0].text,
            "second",
            "the first mod lacks entities, so the second owns non-player rows"
        );
        assert_eq!(merged.tint(7, true), Some([1., 0., 0., 1.]));
        assert_eq!(merged.tint(7, false), Some([0., 1., 0., 1.]));
        assert_eq!(&*merged.roster[&7].spans[0].text, "tab");
        assert_eq!(merged.hurt_overlay.body, Some([0., 0., 1., 1.]));
        assert_eq!(merged.chat_actions, second.chat_actions);
        let lines: Vec<_> = merged
            .tooltip_lines("minecraft:apple", 1)
            .map(|line| line.text.as_str())
            .collect();
        assert_eq!(lines, ["first", "second"]);
        assert_eq!(merged.tooltip_lines("minecraft:apple", 0).count(), 1);
        let reordered = Presentation::merge([(both, &second), (PLAYERS, &first)]);
        assert_eq!(
            &*reordered.nametag(7, true).unwrap().spans[0].text,
            "second"
        );
        assert!(reordered.nametag_style.as_ref().unwrap().scale == 1.);
    }

    /// Rows a mod's grants do not admit never apply, and roster rows require players.
    #[test]
    fn actor_rows_follow_each_mods_grants() {
        let output = ModPresentation {
            annotations: BTreeMap::from([(
                3,
                Annotation {
                    nametag: Some(spans("mob", SpanPosition::Above)),
                    roster: Some(spans("tab", SpanPosition::After)),
                },
            )]),
            tints: BTreeMap::from([(3, [1.; 4])]),
            ..Default::default()
        };
        let merged = Presentation::merge([(ENTITIES, &output)]);
        assert!(merged.nametag(3, true).is_none());
        assert!(merged.nametag(3, false).is_some());
        assert!(merged.roster.is_empty());
        assert!(merged.tint(3, true).is_none());
        let none = Presentation::merge([(ActorGrants::default(), &output)]);
        assert!(none.nametags.is_empty() && none.tints.is_empty());
    }

    /// Each item tooltip receives a bounded number of mod lines.
    #[test]
    fn item_tooltips_are_bounded_across_mods() {
        let line = TooltipLine {
            identifier: "minecraft:bread".into(),
            metadata: None,
            text: "line".into(),
            color: [1.; 4],
        };
        let output = ModPresentation {
            tooltip_lines: vec![line; MAX_ITEM_TOOLTIP_LINES + 3],
            ..Default::default()
        };
        let merged = Presentation::merge([(PLAYERS, &output), (PLAYERS, &output)]);
        assert_eq!(
            merged.tooltip_lines("minecraft:bread", 0).count(),
            MAX_ITEM_TOOLTIP_LINES
        );
    }

    /// Validation bounds spans, styles, tooltip lines and chat actions.
    #[test]
    fn validation_rejects_unbounded_or_styled_text() {
        assert!(spans("ok", SpanPosition::After).validate().is_ok());
        assert!(spans("§c", SpanPosition::After).validate().is_err());
        assert!(
            spans(
                &"x".repeat(MAX_ANNOTATION_SPAN_BYTES + 1),
                SpanPosition::After
            )
            .validate()
            .is_err()
        );
        let many = Spans {
            spans: vec![
                Span {
                    text: "a".into(),
                    color: [1.; 4]
                };
                MAX_ANNOTATION_SPANS + 1
            ]
            .into(),
            position: SpanPosition::After,
        };
        assert!(many.validate().is_err());
        let bad_color = Spans {
            spans: [Span {
                text: "a".into(),
                color: [2., 0., 0., 1.],
            }]
            .into(),
            position: SpanPosition::After,
        };
        assert!(bad_color.validate().is_err());
        assert!(
            NametagStyle {
                scale: 4.,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            NametagStyle {
                text_color: Some([f32::NAN; 4]),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(NametagStyle::default().validate().is_ok());
        let action = |text: &str, action: &str| ChatAction {
            text: text.into(),
            action: action.into(),
        };
        assert!(action("Help", "help.open").validate().is_ok());
        assert!(action("Help", "bad id").validate().is_err());
        assert!(action("", "help").validate().is_err());
        let line = |identifier: &str, text: &str| TooltipLine {
            identifier: identifier.into(),
            metadata: None,
            text: text.into(),
            color: [1.; 4],
        };
        assert!(line("minecraft:apple", "Food").validate().is_ok());
        assert!(line("", "Food").validate().is_err());
        assert!(line("minecraft:apple", "a\nb").validate().is_err());
    }
}
