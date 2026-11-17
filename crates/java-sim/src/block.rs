//! Java Edition 1.7.10 blocks as the simulation sees them: numeric identity, material facts and
//! collision boxes.

mod facts;
mod shapes;

pub use facts::{BlockFacts, Material, facts};
pub use shapes::collision_boxes;

/// A block by its 1.7.10 numeric id and 4-bit metadata.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Block {
    pub id: u16,
    pub meta: u8,
}

impl Block {
    /// Air, which is also what unknown ids resolve to.
    pub const AIR: Self = Self { id: 0, meta: 0 };

    /// A block from its numeric id and metadata; metadata keeps its low four bits.
    #[must_use]
    pub const fn new(id: u16, meta: u8) -> Self {
        Self {
            id,
            meta: meta & 0x0F,
        }
    }
}

/// Numeric ids the simulation names directly.
pub mod id {
    pub const LADDER: u16 = 65;
    pub const VINE: u16 = 106;
    pub const WEB: u16 = 30;
    pub const SOUL_SAND: u16 = 88;
    pub const FLOWING_WATER: u16 = 8;
    pub const WATER: u16 = 9;
    pub const FLOWING_LAVA: u16 = 10;
    pub const LAVA: u16 = 11;
    pub const GLASS: u16 = 20;
    pub const STAINED_GLASS: u16 = 95;
}
