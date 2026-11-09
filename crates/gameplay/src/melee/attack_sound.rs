//! The attack sound the local player voices itself on the frame it attacks an actor.
//!
//! Vanilla clients do not wait for the server: the attack plays `attack.nodamage`,
//! `attack.critical` or `attack.strong` locally, and servers send it only to other players.

use crate::movement::MeleeEffects;
use assets::facts::entity::{EntityFact, NativeEntityClass};

/// The local player's base attack damage: the `minecraft:attack` damage of the vanilla
/// `minecraft:player` definition, which the client applies to its own player. The attribute is
/// not synchronized from the server.
pub const PLAYER_ATTACK_DAMAGE: f32 = 1.0;
const STRENGTH_SCALE: f32 = 1.3;
const STRENGTH_BONUS: f32 = 1.0;
const WEAKNESS_SCALE: f32 = 0.8;
const WEAKNESS_PENALTY: f32 = 0.5;

/// One of the player's three attack sounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttackSound {
    NoDamage,
    Critical,
    Strong,
}

impl AttackSound {
    /// The `minecraft:player` level sound event that voices it.
    pub const fn event(self) -> &'static str {
        match self {
            Self::NoDamage => "attack.nodamage",
            Self::Critical => "attack.critical",
            Self::Strong => "attack.strong",
        }
    }
}

/// The attacker's state on the attack frame.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct AttackerState {
    pub effects: MeleeEffects,
    pub fall_distance: f32,
    pub on_ground: bool,
    /// A climbable block occupies the attacker's feet.
    pub on_climbable: bool,
    pub in_water: bool,
    pub riding: bool,
}

/// Chooses the sound for an attack on an actor. A hit that deals no damage sounds as
/// `NoDamage`; a damaging hit while falling on a mob is `Critical`, otherwise `Strong`.
pub fn attack_sound(attacker: AttackerState, target_is_mob: bool) -> AttackSound {
    if attack_damage(attacker.effects) <= 0.0 {
        return AttackSound::NoDamage;
    }
    let critical = target_is_mob
        && attacker.fall_distance > 0.0
        && !attacker.on_ground
        && !attacker.on_climbable
        && !attacker.in_water
        && !attacker.effects.blindness
        && !attacker.riding;
    if critical {
        AttackSound::Critical
    } else {
        AttackSound::Strong
    }
}

/// Whether an actor of this class is a mob, which critical hits require. Players and every
/// living class are; vehicles, projectiles, items and other inanimate classes are not.
pub fn is_mob(fact: &EntityFact) -> bool {
    match fact.native_class {
        NativeEntityClass::Player
        | NativeEntityClass::Horse
        | NativeEntityClass::Creeper
        | NativeEntityClass::EnderDragon
        | NativeEntityClass::Wolf
        | NativeEntityClass::Sheep
        | NativeEntityClass::Tropicalfish
        | NativeEntityClass::Shulker => true,
        // Custom actors without a resolved base and ordinary classes are mostly mobs.
        NativeEntityClass::Unresolved | NativeEntityClass::Ordinary => !fact.projectile,
        NativeEntityClass::Arrow
        | NativeEntityClass::Boat
        | NativeEntityClass::Minecart
        | NativeEntityClass::EnderCrystal
        | NativeEntityClass::FallingBlock
        | NativeEntityClass::Tnt
        | NativeEntityClass::Painting
        | NativeEntityClass::FishingHook
        | NativeEntityClass::LightningBolt
        | NativeEntityClass::Item
        | NativeEntityClass::XpOrb
        | NativeEntityClass::SplashPotion
        | NativeEntityClass::LingeringPotion
        | NativeEntityClass::DragonFireball
        | NativeEntityClass::FireworksRocket
        | NativeEntityClass::ThrownTrident
        | NativeEntityClass::AreaEffectCloud => false,
    }
}

/// The damage the attack deals before the target's armor: base damage scaled by each
/// Strength level and then each Weakness level, floored at zero once Weakness makes it
/// negative. The held item's damage and enchantment bonus are never negative and are not
/// known locally, so they count as zero.
fn attack_damage(effects: MeleeEffects) -> f32 {
    let levels = |amplifier: Option<i32>| {
        amplifier
            .and_then(|amplifier| u32::try_from(amplifier).ok())
            .map_or(0, |amplifier| u64::from(amplifier) + 1)
    };
    let mut damage = PLAYER_ATTACK_DAMAGE;
    for _ in 0..levels(effects.strength) {
        if !damage.is_finite() {
            break;
        }
        damage = damage * STRENGTH_SCALE + STRENGTH_BONUS;
    }
    for _ in 0..levels(effects.weakness) {
        // An unbounded value never falls; any finite one turns negative within a few hundred.
        if damage.is_infinite() {
            break;
        }
        damage = damage * WEAKNESS_SCALE - WEAKNESS_PENALTY;
        if damage < 0.0 {
            return 0.0;
        }
    }
    damage
}

#[cfg(test)]
mod tests {
    use super::*;

    fn falling() -> AttackerState {
        AttackerState {
            fall_distance: 0.4,
            ..AttackerState::default()
        }
    }

    #[test]
    fn standing_hits_are_strong_and_falling_hits_on_mobs_are_critical() {
        let standing = AttackerState {
            on_ground: true,
            ..AttackerState::default()
        };
        assert_eq!(attack_sound(standing, true), AttackSound::Strong);
        assert_eq!(attack_sound(falling(), true), AttackSound::Critical);
        assert_eq!(attack_sound(falling(), false), AttackSound::Strong);
    }

    #[test]
    fn each_critical_condition_alone_makes_a_falling_hit_strong() {
        let blocked = [
            AttackerState {
                fall_distance: 0.0,
                ..falling()
            },
            AttackerState {
                on_ground: true,
                ..falling()
            },
            AttackerState {
                on_climbable: true,
                ..falling()
            },
            AttackerState {
                in_water: true,
                ..falling()
            },
            AttackerState {
                riding: true,
                ..falling()
            },
            AttackerState {
                effects: MeleeEffects {
                    blindness: true,
                    ..MeleeEffects::default()
                },
                ..falling()
            },
        ];
        for attacker in blocked {
            assert_eq!(
                attack_sound(attacker, true),
                AttackSound::Strong,
                "{attacker:?}"
            );
        }
    }

    #[test]
    fn weakness_that_cancels_all_damage_sounds_as_no_damage() {
        let weakened = |weakness, strength| AttackerState {
            effects: MeleeEffects {
                weakness: Some(weakness),
                strength,
                blindness: false,
            },
            ..falling()
        };
        // 1 * 0.8 - 0.5 still damages; a second level falls below zero.
        assert_eq!(attack_sound(weakened(0, None), true), AttackSound::Critical);
        assert_eq!(attack_sound(weakened(1, None), true), AttackSound::NoDamage);
        assert_eq!(
            attack_sound(weakened(1, None), false),
            AttackSound::NoDamage
        );
        // Strength applies first: (1 * 1.3 + 1) survives two Weakness levels.
        assert_eq!(
            attack_sound(weakened(1, Some(0)), true),
            AttackSound::Critical
        );
        assert_eq!(
            attack_sound(weakened(i32::MAX, Some(i32::MAX)), true),
            AttackSound::Critical
        );
        assert_eq!(
            attack_sound(weakened(i32::MAX, None), true),
            AttackSound::NoDamage
        );
        // Negative amplifiers apply no levels.
        assert_eq!(
            attack_sound(weakened(-3, None), true),
            AttackSound::Critical
        );
    }

    #[test]
    fn sound_events_name_the_player_attack_sounds() {
        assert_eq!(AttackSound::NoDamage.event(), "attack.nodamage");
        assert_eq!(AttackSound::Critical.event(), "attack.critical");
        assert_eq!(AttackSound::Strong.event(), "attack.strong");
    }

    #[test]
    fn players_and_living_classes_are_mobs_but_vehicles_and_projectiles_are_not() {
        let fact = |native_class, projectile| EntityFact {
            native_class,
            projectile,
            ..EntityFact::default()
        };
        assert!(is_mob(&fact(NativeEntityClass::Player, false)));
        assert!(is_mob(&fact(NativeEntityClass::Ordinary, false)));
        assert!(!is_mob(&fact(NativeEntityClass::Ordinary, true)));
        assert!(!is_mob(&fact(NativeEntityClass::Boat, false)));
        assert!(!is_mob(&fact(NativeEntityClass::Minecart, false)));
    }

    #[test]
    fn base_damage_matches_the_pinned_player_definition() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/assets/bedrock-samples")
            .join(assets::vanilla_source().tag.as_ref())
            .join("full/behavior_pack/entities/player.json");
        let Ok(bytes) = std::fs::read(&path) else {
            eprintln!(
                "skipping base_damage_matches_the_pinned_player_definition: missing player.json fixture at {}",
                path.display()
            );
            return;
        };
        let definition: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let damage = &definition["minecraft:entity"]["components"]["minecraft:attack"]["damage"];
        assert_eq!(damage.as_f64(), Some(f64::from(PLAYER_ATTACK_DAMAGE)));
    }
}
