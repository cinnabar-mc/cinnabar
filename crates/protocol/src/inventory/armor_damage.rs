//! `PlayerArmorDamage`: the server's damage values for the local player's worn armor.
//!
//! A server with server-authoritative inventories sends this instead of an armor slot
//! update when worn armor loses durability. Each entry names an armor slot and that piece's
//! new absolute `Damage` value; the client rewrites the worn stack in place, keeping its
//! stack network ID.

use std::sync::Arc;

use bedrock_protocol_1_26_60::packets::PlayerArmorDamage;

use super::InventoryEvent;

/// One worn piece's new damage value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArmorDamage {
    /// The armor slot as sent: 0 head, 1 chest, 2 legs, 3 feet, 4 body. Other values are
    /// odd server data the inventory skips.
    pub slot: i32,
    /// The piece's absolute `Damage` value, not an amount to add.
    pub damage: i16,
}

/// Every damaged piece one packet reports, in wire order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArmorDamageEvent {
    pub pieces: Arc<[ArmorDamage]>,
}

/// Converts the packet's slot and damage pairs without interpreting them.
pub(crate) fn normalize_armor_damage(packet: PlayerArmorDamage) -> InventoryEvent {
    InventoryEvent::ArmorDamage(ArmorDamageEvent {
        pieces: packet
            .armor_slot_and_damage_pairs
            .into_iter()
            .map(|pair| ArmorDamage {
                slot: pair.armor_slot.to_raw(),
                damage: pair.damage.0,
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use bedrock_protocol_1_26_60::enums::LegacyArmorSlot;
    use bedrock_protocol_1_26_60::types::ArmorSlotAndDamagePair;
    use bedrock_protocol_1_26_60::wire;

    use super::*;

    #[test]
    fn armor_damage_keeps_each_slot_and_absolute_damage_in_order() {
        let packet = PlayerArmorDamage {
            armor_slot_and_damage_pairs: vec![
                ArmorSlotAndDamagePair {
                    armor_slot: LegacyArmorSlot::Torso,
                    damage: wire::I16LE(12),
                },
                ArmorSlotAndDamagePair {
                    armor_slot: LegacyArmorSlot::Unknown(9),
                    damage: wire::I16LE(3),
                },
            ],
        };
        assert_eq!(
            normalize_armor_damage(packet),
            InventoryEvent::ArmorDamage(ArmorDamageEvent {
                pieces: Arc::from([
                    ArmorDamage {
                        slot: 1,
                        damage: 12
                    },
                    ArmorDamage { slot: 9, damage: 3 },
                ]),
            })
        );
    }
}
