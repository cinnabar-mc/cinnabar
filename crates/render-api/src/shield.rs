//! Immutable cloth colors and mask identities shared by shield drawing routes.
use std::sync::Arc;

/// Number of ordered banner overlays displayed on a shield.
pub const SHIELD_PATTERN_LAYERS: usize = 6;

/// One banner mask and dye in its authored order.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BannerPattern {
    pub pattern: Arc<str>,
    pub color: u8,
}

/// The cloth base and visible masks of a decorated shield.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ShieldPattern {
    pub base: u8,
    pub layers: Arc<[BannerPattern]>,
}

impl ShieldPattern {
    /// Applies display defaults and the visible-layer count to decoded metadata.
    /// An absent or sentinel base with no overlays leaves the shield undecorated.
    #[must_use]
    pub fn new(base: Option<i32>, layers: impl IntoIterator<Item = BannerPattern>) -> Option<Self> {
        let layers: Arc<[_]> = layers.into_iter().take(SHIELD_PATTERN_LAYERS).collect();
        if layers.is_empty() && base.is_none_or(|base| base == -1) {
            return None;
        }
        Some(Self {
            base: match base {
                Some(-1) => 15,
                Some(value) => value as u8,
                None => 0,
            },
            layers,
        })
    }
}
