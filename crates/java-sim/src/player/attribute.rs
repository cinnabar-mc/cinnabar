//! The player's `generic.movementSpeed` attribute: a base value and its modifiers.

/// Identifier of the modifier the client applies while sprinting.
pub const SPRINT_MODIFIER_ID: u128 = 0x662A_6B8D_DA3E_4C1C_8813_96EA_6097_278D;
/// The sprint modifier multiplies the total by `1 + 0.3f`.
const SPRINT_MODIFIER_AMOUNT: f64 = 0.3_f32 as f64;
/// Base value of a player's movement speed before the server reports one.
const DEFAULT_BASE: f64 = 0.1_f32 as f64;

/// How a modifier combines with the attribute value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    /// Added to the base value.
    Add,
    /// Adds `amount` times the summed base.
    MultiplyBase,
    /// Multiplies the running total by `1 + amount`.
    MultiplyTotal,
}

impl Operation {
    /// The operation for a wire value, or `None` for values the game has no operation for.
    #[must_use]
    pub const fn from_wire(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Add),
            1 => Some(Self::MultiplyBase),
            2 => Some(Self::MultiplyTotal),
            _ => None,
        }
    }
}

/// One attribute modifier, identified by its UUID.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Modifier {
    pub id: u128,
    pub amount: f64,
    pub operation: Operation,
}

/// Movement speed as the client holds it: the server's last snapshot plus the sprint modifier
/// the client toggles itself.
#[derive(Clone, Debug, PartialEq)]
pub struct MovementSpeed {
    base: f64,
    modifiers: Vec<Modifier>,
}

impl Default for MovementSpeed {
    fn default() -> Self {
        Self {
            base: DEFAULT_BASE,
            modifiers: Vec::new(),
        }
    }
}

impl MovementSpeed {
    /// Replaces the base value and every modifier with a server snapshot. A snapshot sent while
    /// the server does not consider the player sprinting drops the local sprint modifier until
    /// the client next changes its sprint state, as in the game. Repeated ids keep the first.
    pub fn apply_snapshot(&mut self, base: f64, modifiers: &[Modifier]) {
        self.base = base;
        self.modifiers.clear();
        for modifier in modifiers {
            if self.modifiers.iter().all(|held| held.id != modifier.id) {
                self.modifiers.push(*modifier);
            }
        }
    }

    /// Removes the sprint modifier, then applies it again when `sprinting`.
    pub fn set_sprinting(&mut self, sprinting: bool) {
        self.modifiers
            .retain(|modifier| modifier.id != SPRINT_MODIFIER_ID);
        if sprinting {
            self.modifiers.push(Modifier {
                id: SPRINT_MODIFIER_ID,
                amount: SPRINT_MODIFIER_AMOUNT,
                operation: Operation::MultiplyTotal,
            });
        }
    }

    /// The attribute value: additions, then base multipliers, then total multipliers, clamped to
    /// be non-negative. Modifiers of one operation combine in the order of the game's hash set,
    /// which is that of Java 8 and later for the up to twelve modifiers a default table holds.
    #[must_use]
    pub fn value(&self) -> f64 {
        let mut base = self.base;
        for modifier in self.ordered(Operation::Add) {
            base += modifier.amount;
        }
        let mut total = base;
        for modifier in self.ordered(Operation::MultiplyBase) {
            total += base * modifier.amount;
        }
        for modifier in self.ordered(Operation::MultiplyTotal) {
            total *= 1.0 + modifier.amount;
        }
        if total < 0.0 { 0.0 } else { total }
    }

    /// Modifiers of one operation in hash-bucket order; equal buckets keep insertion order.
    fn ordered(&self, operation: Operation) -> Vec<Modifier> {
        let mut selected: Vec<Modifier> = self
            .modifiers
            .iter()
            .copied()
            .filter(|modifier| modifier.operation == operation)
            .collect();
        selected.sort_by_key(|modifier| hash_bucket(modifier.id));
        selected
    }
}

/// Bucket of a UUID in a 16-slot Java hash table: the UUID's hash folds its four 32-bit words,
/// and the table mixes the high half into the low half.
fn hash_bucket(id: u128) -> u32 {
    let folded = (id >> 64) as u64 ^ id as u64;
    let hash = (folded >> 32) as u32 ^ folded as u32;
    (hash ^ (hash >> 16)) & 15
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sprinting_multiplies_the_base_speed() {
        let mut speed = MovementSpeed::default();
        assert_eq!(speed.value(), f64::from(0.1_f32));
        speed.set_sprinting(true);
        assert_eq!(
            speed.value(),
            f64::from(0.1_f32) * (1.0 + f64::from(0.3_f32))
        );
        speed.set_sprinting(true);
        assert_eq!(speed.modifiers.len(), 1);
        speed.set_sprinting(false);
        assert_eq!(speed.value(), f64::from(0.1_f32));
    }

    #[test]
    fn a_snapshot_without_the_sprint_modifier_drops_it() {
        let mut speed = MovementSpeed::default();
        speed.set_sprinting(true);
        speed.apply_snapshot(0.1, &[]);
        assert_eq!(speed.value(), 0.1);
    }

    #[test]
    fn operations_apply_in_order() {
        let mut speed = MovementSpeed::default();
        speed.apply_snapshot(
            1.0,
            &[
                Modifier {
                    id: 1,
                    amount: 0.5,
                    operation: Operation::MultiplyTotal,
                },
                Modifier {
                    id: 2,
                    amount: 1.0,
                    operation: Operation::Add,
                },
                Modifier {
                    id: 3,
                    amount: 0.25,
                    operation: Operation::MultiplyBase,
                },
                Modifier {
                    id: 2,
                    amount: 9.0,
                    operation: Operation::Add,
                },
            ],
        );
        assert_eq!(speed.value(), (2.0 + 2.0 * 0.25) * 1.5);
    }
}
