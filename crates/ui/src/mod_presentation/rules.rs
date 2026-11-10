//! Ordered presentation rules mods set for every actor a selector covers: nametag visibility and
//! overlay tints.

use super::{ActorGrants, Rgba, validate_color};

/// Most nametag visibility rules one mod retains.
pub const MAX_NAMETAG_RULES: usize = 128;
/// Most tint rules one mod retains.
pub const MAX_TINT_RULES: usize = 128;

/// The facts about one actor that selectors read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ActorFacts {
    pub runtime_id: u64,
    pub player: bool,
    /// The local player's own actor.
    pub local: bool,
    /// The actor rides another actor.
    pub riding: bool,
}

/// Which actors a rule covers, by facts the client knows about them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ActorSelector {
    All,
    /// Players, the local player included.
    Players,
    NonPlayers,
    LocalPlayer,
    /// Actors riding another actor.
    Riding,
    /// One actor by nonzero runtime ID, among the kinds the rule owner's grants admit.
    Actor(u64),
}

impl ActorSelector {
    /// Whether this selector covers `actor` for a rule owner with `grants`.
    pub fn matches(self, actor: &ActorFacts, grants: ActorGrants) -> bool {
        match self {
            Self::All => true,
            Self::Players => actor.player,
            Self::NonPlayers => !actor.player,
            Self::LocalPlayer => actor.local,
            Self::Riding => actor.riding,
            Self::Actor(runtime_id) => {
                runtime_id == actor.runtime_id
                    && if actor.player {
                        grants.players
                    } else {
                        grants.entities
                    }
            }
        }
    }

    /// Rejects runtime ID zero.
    pub fn validate(self) -> Result<(), String> {
        if self == Self::Actor(0) {
            return Err("actor selectors need nonzero runtime IDs".into());
        }
        Ok(())
    }
}

/// Whether one actor's nametag draws.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NametagVisibility {
    /// As the client draws it: the local player's own tag only when the player's settings or
    /// an annotation show it.
    #[default]
    Native,
    /// Also the local player's own tag.
    Shown,
    /// Not drawn.
    Hidden,
}

/// One nametag visibility rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NametagRule {
    pub actors: ActorSelector,
    /// Applies only while the player has hidden the HUD (true) or shows it (false); none
    /// applies either way.
    pub hud_hidden: Option<bool>,
    pub visibility: NametagVisibility,
}

/// The visibility `rules` give `actor`: the client's own rule first, hiding every tag while
/// the HUD is hidden, then each matching rule in order, the last one deciding.
pub fn nametag_visibility(
    rules: &[NametagRule],
    grants: ActorGrants,
    actor: &ActorFacts,
    hud_hidden: bool,
) -> NametagVisibility {
    let initial = if hud_hidden {
        NametagVisibility::Hidden
    } else {
        NametagVisibility::Native
    };
    rules
        .iter()
        .filter(|rule| rule.hud_hidden.is_none_or(|hidden| hidden == hud_hidden))
        .filter(|rule| rule.actors.matches(actor, grants))
        .fold(initial, |_, rule| rule.visibility)
}

/// The actor state a tint applies in. `Hurt` follows the native damage flash.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TintState {
    Hurt,
    NotHurt,
}

impl TintState {
    /// The state of an actor that is or is not hurt.
    pub fn of(hurt: bool) -> Self {
        if hurt { Self::Hurt } else { Self::NotHurt }
    }
}

/// Overlay colours for an actor's body and its armour; an absent part keeps native.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Tint {
    pub body: Option<Rgba>,
    pub armor: Option<Rgba>,
}

impl Tint {
    /// Requires at least one valid colour.
    pub fn validate(&self) -> Result<(), String> {
        if self.body.is_none() && self.armor.is_none() {
            return Err("a tint needs a body or armour colour".into());
        }
        [self.body, self.armor]
            .into_iter()
            .flatten()
            .try_for_each(validate_color)
    }

    /// The overlay colour for one draw layer: armour layers take `armor`; other layers take
    /// `body`, which while hurt only replaces a native damage overlay the layer draws.
    pub fn overlay(&self, state: TintState, armor: bool, native_overlay: bool) -> Option<Rgba> {
        if armor {
            self.armor
        } else if state == TintState::Hurt && !native_overlay {
            None
        } else {
            self.body
        }
    }
}

/// One tint rule.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TintRule {
    pub actors: ActorSelector,
    pub state: TintState,
    pub tint: Tint,
}

/// The tint the last matching rule for `state` gives `actor`, if any.
pub fn tint(
    rules: &[TintRule],
    grants: ActorGrants,
    actor: &ActorFacts,
    state: TintState,
) -> Option<Tint> {
    rules
        .iter()
        .rev()
        .find(|rule| rule.state == state && rule.actors.matches(actor, grants))
        .map(|rule| rule.tint)
}

#[cfg(test)]
#[path = "rules_tests.rs"]
mod tests;
