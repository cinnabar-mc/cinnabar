//! Armor trims before and after composition; captured only with the client test support.

use super::*;

/// Untrimmed and trimmed armor at the same viewport.
pub(super) const STATES: &[State] = &[
    #[cfg(feature = "test-support")]
    State {
        name: "armor-trims-before",
        size: [576, 672],
        gui_scale: 1,
        fixture: || Fixture::ArmorTrims { trimmed: false },
    },
    #[cfg(feature = "test-support")]
    State {
        name: "armor-trims-after",
        size: [576, 672],
        gui_scale: 1,
        fixture: || Fixture::ArmorTrims { trimmed: true },
    },
];
