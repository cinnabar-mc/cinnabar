//! Category clocks shared by uses in air and on a block.
use super::{Cooldown, ItemUseRuntime};

impl ItemUseRuntime {
    /// Allows use at the exact tick the selected category expires, including between air-use frames.
    pub fn cooldown_ready(&self, cooldown: Cooldown, tick: u64) -> bool {
        !self
            .cooldowns
            .iter()
            .any(|(category, until)| *category == cooldown.category && tick < *until)
    }

    /// Starts an admitted item's category clock without creating duplicate timers for that category.
    pub fn start_cooldown(&mut self, cooldown: Cooldown, tick: u64) {
        self.cooldowns
            .retain(|(category, _)| *category != cooldown.category);
        if cooldown.ticks > 0 {
            self.cooldowns.push((
                cooldown.category,
                tick.saturating_add(u64::from(cooldown.ticks)),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A use on a block shares its timer with air use and expires at the original tick deadline.
    #[test]
    fn block_use_cooldowns_share_categories_and_preserve_the_expiry_tick() {
        let mut runtime = ItemUseRuntime::default();
        let cooldown = Cooldown {
            category: client_world::UseCooldownCategory {
                generation: 1,
                index: 0,
            },
            ticks: 6,
        };
        runtime.start_cooldown(cooldown, 101);
        assert!(!runtime.cooldown_ready(cooldown, 106));
        assert!(runtime.cooldown_ready(cooldown, 107));
        assert!(!runtime.cooldown_ready(
            Cooldown {
                ticks: 40,
                ..cooldown
            },
            106
        ));
        assert!(runtime.cooldown_ready(
            Cooldown {
                category: client_world::UseCooldownCategory {
                    generation: 2,
                    index: 0
                },
                ..cooldown
            },
            106
        ));
        runtime.start_cooldown(cooldown, 107);
        assert_eq!(runtime.cooldowns, [(cooldown.category, 113)]);
    }
}
