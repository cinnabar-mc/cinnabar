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

/// Smallest table a Java hash set starts with.
const INITIAL_BUCKETS: u32 = 16;

/// Movement speed as the client holds it: the server's last snapshot plus the sprint modifier
/// the client toggles itself.
#[derive(Clone, Debug, PartialEq)]
pub struct MovementSpeed {
    base: f64,
    modifiers: Vec<Modifier>,
    /// Bucket count of the game's hash set for each operation. A set doubles when it grows
    /// past three quarters full and never shrinks, so the count depends on its history.
    buckets: [u32; 3],
}

impl Default for MovementSpeed {
    fn default() -> Self {
        Self {
            base: DEFAULT_BASE,
            modifiers: Vec::new(),
            buckets: [INITIAL_BUCKETS; 3],
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
                self.insert(*modifier);
            }
        }
    }

    /// Removes the sprint modifier, then applies it again when `sprinting`.
    pub fn set_sprinting(&mut self, sprinting: bool) {
        self.modifiers
            .retain(|modifier| modifier.id != SPRINT_MODIFIER_ID);
        if sprinting {
            self.insert(Modifier {
                id: SPRINT_MODIFIER_ID,
                amount: SPRINT_MODIFIER_AMOUNT,
                operation: Operation::MultiplyTotal,
            });
        }
    }

    /// Adds a modifier and grows its operation's table as the game's hash set would.
    fn insert(&mut self, modifier: Modifier) {
        self.modifiers.push(modifier);
        let held = self
            .modifiers
            .iter()
            .filter(|held| held.operation == modifier.operation)
            .count();
        let buckets = &mut self.buckets[modifier.operation as usize];
        while held as u64 * 4 > u64::from(*buckets) * 3 && *buckets < 1 << 30 {
            *buckets *= 2;
        }
    }

    /// The attribute value: additions, then base multipliers, then total multipliers, clamped to
    /// be non-negative. Modifiers of one operation combine in the iteration order of the game's
    /// hash set under Java 8 and later: by bucket, then by insertion. Buckets holding eight or
    /// more modifiers, which Java reorders into a tree, are not modelled.
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
        let buckets = self.buckets[operation as usize];
        let mut selected: Vec<Modifier> = self
            .modifiers
            .iter()
            .copied()
            .filter(|modifier| modifier.operation == operation)
            .collect();
        selected.sort_by_key(|modifier| hash_bucket(modifier.id, buckets));
        selected
    }
}

/// Bucket of a UUID in a Java hash table of `buckets` slots: the UUID's hash folds its four
/// 32-bit words, and the table mixes the high half into the low half.
fn hash_bucket(id: u128, buckets: u32) -> u32 {
    let folded = (id >> 64) as u64 ^ id as u64;
    let hash = (folded >> 32) as u32 ^ folded as u32;
    (hash ^ (hash >> 16)) & (buckets - 1)
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

    /// Thirteen additions outgrow a 16-slot table. Ids 0, 16 and 1 share slots 0, 0 and 1
    /// there but take slots 0, 16 and 1 once the table has doubled, which changes the order
    /// of a sum that is not associative.
    #[test]
    fn a_table_that_grew_orders_modifiers_by_its_new_size() {
        let add = |id: u128, amount: f64| Modifier {
            id,
            amount,
            operation: Operation::Add,
        };
        let mut modifiers = vec![add(0, 1e16), add(16, -1e16), add(1, 1.0)];
        modifiers.extend((2..12).map(|id| add(id, 0.0)));
        let mut speed = MovementSpeed::default();
        speed.apply_snapshot(0.0, &modifiers[..12]);
        assert_eq!(speed.value(), 1.0, "twelve modifiers fit sixteen slots");
        speed.apply_snapshot(0.0, &modifiers);
        assert_eq!(speed.value(), 0.0, "thirteen modifiers double the table");
        // The table keeps its size when the set empties.
        speed.apply_snapshot(0.0, &modifiers[..3]);
        assert_eq!(speed.value(), 0.0);
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
