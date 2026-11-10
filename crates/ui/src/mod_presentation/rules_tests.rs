use super::*;

const BOTH: ActorGrants = ActorGrants {
    players: true,
    entities: true,
};

/// Builds one nametag rule.
fn rule(
    actors: ActorSelector,
    hud_hidden: Option<bool>,
    visibility: NametagVisibility,
) -> NametagRule {
    NametagRule {
        actors,
        hud_hidden,
        visibility,
    }
}

/// The nametag style's former selection flags.
#[derive(Clone, Copy, Debug, Default)]
struct LegacyFlags {
    hide: bool,
    hide_players: bool,
    hide_passengers: bool,
    show_self: bool,
    show_when_hud_hidden: bool,
}

impl LegacyFlags {
    /// All 32 combinations.
    fn all() -> impl Iterator<Item = Self> {
        (0..32_u8).map(|bits| Self {
            hide: bits & 1 != 0,
            hide_players: bits & 2 != 0,
            hide_passengers: bits & 4 != 0,
            show_self: bits & 8 != 0,
            show_when_hud_hidden: bits & 16 != 0,
        })
    }

    /// The rule list a mod sends for these flags.
    fn rules(self) -> Vec<NametagRule> {
        use ActorSelector::*;
        use NametagVisibility::*;
        let mut rules = Vec::new();
        if self.show_when_hud_hidden {
            rules.push(rule(All, Some(true), Native));
        }
        if self.show_self {
            rules.push(rule(LocalPlayer, None, Shown));
            if !self.show_when_hud_hidden {
                rules.push(rule(All, Some(true), Hidden));
            }
        }
        if self.hide_passengers {
            rules.push(rule(Riding, None, Hidden));
        }
        if self.hide_players {
            rules.push(rule(Players, None, Hidden));
        }
        if self.hide {
            rules.push(rule(All, None, Hidden));
        }
        rules
    }

    /// Whether the former host policy drew this actor's tag outside menus and first person,
    /// where `own_native` is whether the player's settings or an annotation showed the own tag.
    fn drawn(self, actor: &ActorFacts, hud_hidden: bool, own_native: bool) -> bool {
        if self.hide
            || (hud_hidden && !self.show_when_hud_hidden)
            || (actor.player && self.hide_players)
            || (actor.riding && self.hide_passengers)
        {
            return false;
        }
        !actor.local || self.show_self || own_native
    }
}

/// Whether a tag draws under `visibility`, as the client applies it.
fn drawn(visibility: NametagVisibility, actor: &ActorFacts, own_native: bool) -> bool {
    match visibility {
        NametagVisibility::Native => !actor.local || own_native,
        NametagVisibility::Shown => true,
        NametagVisibility::Hidden => false,
    }
}

/// Every actor shape the selectors distinguish.
fn actors() -> impl Iterator<Item = ActorFacts> {
    (0..8_u8).filter(|bits| bits & 1 != 0 || bits & 2 == 0).map(|bits| ActorFacts {
        runtime_id: 7,
        player: bits & 1 != 0,
        local: bits & 2 != 0,
        riding: bits & 4 != 0,
    })
}

/// Every combination of the former flags is a rule list drawing exactly the same tags.
#[test]
fn every_former_nametag_flag_combination_is_a_rule_list() {
    for flags in LegacyFlags::all() {
        let rules = flags.rules();
        for actor in actors() {
            for hud_hidden in [false, true] {
                for own_native in [false, true] {
                    assert_eq!(
                        drawn(
                            nametag_visibility(&rules, ActorGrants::default(), &actor, hud_hidden),
                            &actor,
                            own_native
                        ),
                        flags.drawn(&actor, hud_hidden, own_native),
                        "{flags:?} {actor:?} hud hidden {hud_hidden} own native {own_native}"
                    );
                }
            }
        }
    }
}

/// The last matching rule decides; conditions and runtime IDs narrow which rules match.
#[test]
fn nametag_rules_apply_in_order_with_conditions_and_grants() {
    use ActorSelector::*;
    use NametagVisibility::*;
    let player = ActorFacts {
        runtime_id: 3,
        player: true,
        ..Default::default()
    };
    assert_eq!(nametag_visibility(&[], BOTH, &player, false), Native);
    assert_eq!(nametag_visibility(&[], BOTH, &player, true), Hidden);
    let rules = [
        rule(Players, None, Hidden),
        rule(Actor(3), Some(false), Shown),
    ];
    assert_eq!(nametag_visibility(&rules, BOTH, &player, false), Shown);
    assert_eq!(nametag_visibility(&rules, BOTH, &player, true), Hidden);
    let entities_only = ActorGrants {
        players: false,
        entities: true,
    };
    assert_eq!(
        nametag_visibility(&rules, entities_only, &player, false),
        Hidden,
        "a runtime ID rule reaches only the kinds its owner's grants admit"
    );
    assert!(Actor(0).validate().is_err() && Actor(1).validate().is_ok());
}

/// The former hurt overlay and per-actor tints are tint rules giving every layer the same
/// overlay.
#[test]
fn former_hurt_overlay_and_actor_tints_are_tint_rules() {
    let colors = [None, Some([1., 0., 0., 0.4]), Some([0., 0., 1., 0.4])];
    let tint_color = [0., 1., 0., 0.3];
    for body in colors {
        for armor in colors {
            let mut rules = Vec::new();
            if body.is_some() || armor.is_some() {
                rules.push(TintRule {
                    actors: ActorSelector::All,
                    state: TintState::Hurt,
                    tint: Tint { body, armor },
                });
            }
            rules.push(TintRule {
                actors: ActorSelector::Actor(7),
                state: TintState::NotHurt,
                tint: Tint {
                    body: Some(tint_color),
                    armor: Some(tint_color),
                },
            });
            for actor in actors().flat_map(|actor| {
                [7, 8].map(|runtime_id| ActorFacts {
                    runtime_id,
                    ..actor
                })
            }) {
                for (hurt, armor_layer, native) in (0..8_u8)
                    .map(|bits| (bits & 1 != 0, bits & 2 != 0, bits & 4 != 0))
                {
                    // The former selection: damage colours while hurt, else the actor's tint.
                    let former = if hurt {
                        if armor_layer {
                            armor
                        } else if native {
                            body
                        } else {
                            None
                        }
                    } else {
                        (actor.runtime_id == 7).then_some(tint_color)
                    };
                    let state = TintState::of(hurt);
                    let current = tint(&rules, BOTH, &actor, state)
                        .and_then(|tint| tint.overlay(state, armor_layer, native));
                    assert_eq!(current, former, "{actor:?} {hurt} {armor_layer} {native}");
                }
            }
        }
    }
}

/// The last matching rule for the actor's state decides its tint.
#[test]
fn tint_rules_select_by_state_and_order() {
    let red = Tint {
        body: Some([1., 0., 0., 1.]),
        armor: None,
    };
    let blue = Tint {
        body: Some([0., 0., 1., 1.]),
        armor: None,
    };
    let rules = [
        TintRule {
            actors: ActorSelector::All,
            state: TintState::Hurt,
            tint: red,
        },
        TintRule {
            actors: ActorSelector::Players,
            state: TintState::Hurt,
            tint: blue,
        },
    ];
    let mob = ActorFacts::default();
    let player = ActorFacts {
        player: true,
        ..mob
    };
    assert_eq!(tint(&rules, BOTH, &mob, TintState::Hurt), Some(red));
    assert_eq!(tint(&rules, BOTH, &player, TintState::Hurt), Some(blue));
    assert_eq!(tint(&rules, BOTH, &player, TintState::NotHurt), None);
    assert!(Tint::default().validate().is_err() && red.validate().is_ok());
    assert_eq!(red.overlay(TintState::Hurt, false, false), None);
    assert_eq!(red.overlay(TintState::NotHurt, false, false), red.body);
}
