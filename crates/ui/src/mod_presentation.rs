//! Parameters mods set on native world presentation, and their deterministic merge.
//!
//! Each mod retains one [`ModPresentation`]. The client merges every active mod's output in load
//! order into one [`Presentation`]: the earliest mod that sets a single policy (nametag style,
//! nametag rules, the tint rules of one actor state, chat actions) owns it; annotations are
//! owned per runtime ID and surface by the earliest mod whose grants admit that actor's kind;
//! tooltip lines are appended in load order.

mod rules;
pub use rules::{
    ActorFacts, ActorSelector, MAX_NAMETAG_RULES, MAX_TINT_RULES, NametagRule, NametagVisibility,
    Tint, TintRule, TintState,
};

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
    pub nametag_rules: Vec<NametagRule>,
    pub annotations: BTreeMap<u64, Annotation>,
    pub tint_rules: Vec<TintRule>,
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
    /// The owning mod's grants and nametag rules.
    pub nametag_rules: (ActorGrants, Vec<NametagRule>),
    pub nametags: HashMap<u64, ByKind<Arc<Spans>>>,
    /// Player-list spans by player runtime ID; only mods with the players grant contribute.
    pub roster: HashMap<u64, Arc<Spans>>,
    /// For each [`TintState`] in order, its owning mod's grants and rules for that state.
    pub tint_rules: [(ActorGrants, Vec<TintRule>); 2],
    pub tooltip_lines: Vec<TooltipLine>,
    pub chat_actions: Vec<ChatAction>,
}

impl Presentation {
    /// Merges mods in load order with the actor kinds each one's grants admit.
    pub fn merge<'a>(mods: impl IntoIterator<Item = (ActorGrants, &'a ModPresentation)>) -> Self {
        let mut merged = Self::default();
        for (grants, output) in mods {
            if merged.nametag_style.is_none() {
                merged.nametag_style.clone_from(&output.nametag_style);
            }
            if merged.nametag_rules.1.is_empty() {
                merged.nametag_rules = (grants, output.nametag_rules.clone());
            }
            for (state, owner) in [TintState::Hurt, TintState::NotHurt]
                .into_iter()
                .zip(&mut merged.tint_rules)
            {
                if owner.1.is_empty() {
                    owner.0 = grants;
                    owner
                        .1
                        .extend(output.tint_rules.iter().filter(|rule| rule.state == state));
                }
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

    /// The nametag visibility the owning mod's rules give `actor`.
    pub fn nametag_visibility(&self, actor: &ActorFacts, hud_hidden: bool) -> NametagVisibility {
        let (grants, rules) = &self.nametag_rules;
        rules::nametag_visibility(rules, *grants, actor, hud_hidden)
    }

    /// Whether some rule can draw a tag while the HUD is hidden.
    pub fn shows_nametags_with_hidden_hud(&self) -> bool {
        self.nametag_rules.1.iter().any(|rule| {
            rule.hud_hidden != Some(false) && rule.visibility != NametagVisibility::Hidden
        })
    }

    /// The tint the owning mod's rules for `state` give `actor`.
    pub fn tint(&self, actor: &ActorFacts, state: TintState) -> Option<Tint> {
        let (grants, rules) = &self.tint_rules[state as usize];
        rules::tint(rules, *grants, actor, state)
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

    /// A rule tinting the bodies of `actors` in `state`.
    fn tint_rule(actors: ActorSelector, state: TintState, color: Rgba) -> TintRule {
        TintRule {
            actors,
            state,
            tint: Tint {
                body: Some(color),
                armor: None,
            },
        }
    }

    /// Facts of a remote actor of this kind.
    fn actor(runtime_id: u64, player: bool) -> ActorFacts {
        ActorFacts {
            runtime_id,
            player,
            ..Default::default()
        }
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
            tint_rules: vec![tint_rule(
                ActorSelector::Actor(7),
                TintState::NotHurt,
                [1., 0., 0., 1.],
            )],
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
            nametag_rules: vec![NametagRule {
                actors: ActorSelector::All,
                hud_hidden: None,
                visibility: NametagVisibility::Hidden,
            }],
            tint_rules: vec![
                tint_rule(ActorSelector::All, TintState::Hurt, [0., 0., 1., 1.]),
                tint_rule(
                    ActorSelector::Actor(7),
                    TintState::NotHurt,
                    [0., 1., 0., 1.],
                ),
            ],
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
            ..Default::default()
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
        let tint = |merged: &Presentation, actor: ActorFacts, state| {
            merged.tint(&actor, state).and_then(|tint| tint.body)
        };
        assert_eq!(
            tint(&merged, actor(7, true), TintState::NotHurt),
            Some([1., 0., 0., 1.])
        );
        assert_eq!(
            tint(&merged, actor(7, false), TintState::NotHurt),
            None,
            "the first mod owns not-hurt tints, and its grants do not admit other actors"
        );
        assert_eq!(&*merged.roster[&7].spans[0].text, "tab");
        assert_eq!(
            tint(&merged, actor(7, true), TintState::Hurt),
            Some([0., 0., 1., 1.]),
            "the second mod owns hurt tints, which the first left unset"
        );
        assert_eq!(
            merged.nametag_visibility(&actor(7, true), false),
            NametagVisibility::Hidden
        );
        assert!(!merged.shows_nametags_with_hidden_hud());
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
            nametag_rules: vec![NametagRule {
                actors: ActorSelector::Actor(3),
                hud_hidden: None,
                visibility: NametagVisibility::Hidden,
            }],
            tint_rules: vec![tint_rule(
                ActorSelector::Actor(3),
                TintState::NotHurt,
                [1.; 4],
            )],
            ..Default::default()
        };
        let merged = Presentation::merge([(ENTITIES, &output)]);
        assert_eq!(
            merged.nametag_visibility(&actor(3, false), false),
            NametagVisibility::Hidden
        );
        assert_eq!(
            merged.nametag_visibility(&actor(3, true), false),
            NametagVisibility::Native
        );
        assert!(merged.nametag(3, true).is_none());
        assert!(merged.nametag(3, false).is_some());
        assert!(merged.roster.is_empty());
        assert!(merged.tint(&actor(3, true), TintState::NotHurt).is_none());
        assert!(merged.tint(&actor(3, false), TintState::NotHurt).is_some());
        let none = Presentation::merge([(ActorGrants::default(), &output)]);
        assert!(none.nametags.is_empty());
        assert!(none.tint(&actor(3, false), TintState::NotHurt).is_none());
    }

    /// Only rules that can draw while the HUD is hidden make the client consider tags then.
    #[test]
    fn hidden_hud_tags_need_a_rule_that_can_show_them() {
        let with = |rules: Vec<NametagRule>| {
            Presentation::merge([(
                PLAYERS,
                &ModPresentation {
                    nametag_rules: rules,
                    ..Default::default()
                },
            )])
            .shows_nametags_with_hidden_hud()
        };
        let rule = |hud_hidden, visibility| NametagRule {
            actors: ActorSelector::Players,
            hud_hidden,
            visibility,
        };
        assert!(!with(Vec::new()));
        assert!(with(vec![rule(Some(true), NametagVisibility::Native)]));
        assert!(with(vec![rule(None, NametagVisibility::Shown)]));
        assert!(!with(vec![rule(Some(false), NametagVisibility::Shown)]));
        assert!(!with(vec![rule(None, NametagVisibility::Hidden)]));
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
