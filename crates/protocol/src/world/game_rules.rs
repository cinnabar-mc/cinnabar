//! The world rules the client reads from StartGame and GameRulesChanged.

use bedrock_protocol_1_26_60::packets::StartGame;
use bedrock_protocol_1_26_60::{
    types::{GameRule, GameRuleValue, LevelGameRule, LevelGameRuleValue},
    wire,
};

/// Reads the authoritative `doDaylightCycle` switch from a rule list; only a boolean value counts.
pub(super) fn daylight_cycle_rule_update(rules: &[impl BooleanRule]) -> Option<bool> {
    bool_rule(rules, "dodaylightcycle")
}

/// The `doweathercycle` rule gates vanilla's seasonal palette accumulation.
pub(super) fn weather_cycle_rule_update(rules: &[impl BooleanRule]) -> Option<bool> {
    bool_rule(rules, "doweathercycle")
}

/// Reads boolean values from both startup rules and runtime rule changes.
pub(super) trait BooleanRule {
    /// Returns the rule name and its boolean value, if the declaration is boolean.
    fn boolean(&self) -> (&wire::Str, Option<bool>);
}

impl BooleanRule for GameRule {
    fn boolean(&self) -> (&wire::Str, Option<bool>) {
        (
            &self.rule_name,
            match self.rule_value {
                GameRuleValue::Bool(value) => Some(value),
                _ => None,
            },
        )
    }
}

impl BooleanRule for LevelGameRule {
    fn boolean(&self) -> (&wire::Str, Option<bool>) {
        (
            &self.rule_name,
            match self.rule_value {
                LevelGameRuleValue::Variant1(value) => Some(value),
                _ => None,
            },
        )
    }
}

/// Finds the first boolean declaration whose name matches without case.
fn bool_rule(rules: &[impl BooleanRule], name: &str) -> Option<bool> {
    rules.iter().find_map(|rule| {
        let (rule_name, value) = rule.boolean();
        rule_name
            .as_bytes()
            .eq_ignore_ascii_case(name.as_bytes())
            .then_some(value)
            .flatten()
    })
}

/// Reads the native death controller's two boolean game rules.
pub(super) fn death_rules(rules: &[impl BooleanRule]) -> crate::DeathRules {
    crate::DeathRules {
        show_messages: bool_rule(rules, "showdeathmessages"),
        immediate_respawn: bool_rule(rules, "doimmediaterespawn"),
    }
}

impl crate::DeathRules {
    /// StartGame defaults: show death reasons and wait for a respawn press.
    pub(crate) fn from_start_game(start_game: &StartGame) -> Self {
        let rules = death_rules(&start_game.settings.rule_data.rules_list);
        Self {
            show_messages: Some(rules.show_messages.unwrap_or(Self::DEFAULT_SHOW_MESSAGES)),
            immediate_respawn: Some(
                rules
                    .immediate_respawn
                    .unwrap_or(Self::DEFAULT_IMMEDIATE_RESPAWN),
            ),
        }
    }
}

pub(super) fn hud_rules(rules: &[impl BooleanRule]) -> crate::HudRules {
    crate::HudRules {
        show_coordinates: bool_rule(rules, "showcoordinates"),
        show_days_played: bool_rule(rules, "showdaysplayed"),
    }
}

impl crate::HudRules {
    /// StartGame's HUD rules; an absent rule reads as off, its vanilla default.
    pub(crate) fn from_start_game(start_game: &StartGame) -> Self {
        let rules = hud_rules(&start_game.settings.rule_data.rules_list);
        Self {
            show_coordinates: Some(rules.show_coordinates.unwrap_or(false)),
            show_days_played: Some(rules.show_days_played.unwrap_or(false)),
        }
    }
}

/// Reads only boolean declarations of the two independent crafting rules.
pub(super) fn crafting_rules(rules: &[impl BooleanRule]) -> crate::CraftingRules {
    crate::CraftingRules {
        recipes_unlock: bool_rule(rules, "recipesunlock"),
        limited_crafting: bool_rule(rules, "dolimitedcrafting"),
    }
}

impl crate::CraftingRules {
    /// Retains explicit startup values and resolves omitted rules before play begins.
    pub(crate) fn from_start_game(start_game: &StartGame) -> Self {
        crafting_rules(&start_game.settings.rule_data.rules_list).resolved()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bedrock_protocol_1_26_60::wire;

    #[test]
    fn death_rules_preserve_absent_and_non_boolean_values() {
        let rules = [
            GameRule {
                rule_name: "showDeathMessages".into(),
                rule_can_be_modified: false,
                rule_value: GameRuleValue::Bool(false),
            },
            GameRule {
                rule_name: "DoImmediateRespawn".into(),
                rule_can_be_modified: false,
                rule_value: GameRuleValue::Bool(true),
            },
        ];
        assert_eq!(
            death_rules(&rules),
            crate::DeathRules {
                show_messages: Some(false),
                immediate_respawn: Some(true),
            }
        );
        assert!(death_rules(&[] as &[GameRule]).is_empty());
        let invalid = GameRule {
            rule_value: GameRuleValue::Int32(wire::I32LE(1)),
            ..rules[0].clone()
        };
        assert!(death_rules(&[invalid]).is_empty());
    }

    #[test]
    fn weather_cycle_ignores_unrelated_rules_and_preserves_boolean_updates() {
        let rule = |name: &str, value| GameRule {
            rule_name: name.into(),
            rule_can_be_modified: false,
            rule_value: value,
        };
        assert_eq!(weather_cycle_rule_update(&[] as &[GameRule]), None);
        assert_eq!(
            weather_cycle_rule_update(&[rule(
                "doWeatherCycle",
                GameRuleValue::Int32(wire::I32LE(1))
            )]),
            None
        );
        assert_eq!(
            weather_cycle_rule_update(&[
                rule("doDaylightCycle", GameRuleValue::Bool(false)),
                rule("DoWeatherCycle", GameRuleValue::Bool(false)),
            ]),
            Some(false)
        );
        assert_eq!(
            weather_cycle_rule_update(&[rule("doweathercycle", GameRuleValue::Bool(true))]),
            Some(true)
        );
    }
}
