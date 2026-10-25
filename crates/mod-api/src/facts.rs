//! Limits of the `facts` interface besides its per-callback read allowance,
//! [`crate::MAX_FACT_READS`].

/// Most entries one `facts.read-foods` call returns, the first by identifier. It matches the
/// tooltip line capacity, so a mod can describe every listed food with one line each.
pub const MAX_FOOD_ITEMS: usize = crate::presentation::MAX_TOOLTIP_LINES;
