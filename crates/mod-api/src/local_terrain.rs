//! Bounded observations of a managed local server, independent of its renderer.
use std::sync::Arc;

/// Maximum extent per observation axis.
pub const MAX_EXTENT: u8 = 16;
/// Maximum retained queries across a managed server.
pub const MAX_QUERIES: usize = 4;
/// Maximum palette entries and cells in one observation.
pub const MAX_CELLS: usize = (MAX_EXTENT as usize).pow(3);
/// A cell that could not be observed, distinct from known air.
pub const UNKNOWN: u16 = u16::MAX;
/// Maximum canonical block metadata bytes in one response.
pub const MAX_METADATA_BYTES: usize = 128 * 1024;
pub const MAX_NAME_BYTES: usize = 256;
pub const MAX_STATE_BYTES: usize = 4096;
pub const MAX_COORDINATE: i64 = 30_000_000;
/// The framed companion protocol version.
pub const VERSION: u32 = 1;
/// Script API names in the native Bedrock dimension-number order.
pub const DIMENSIONS: [&str; 3] = ["overworld", "nether", "the_end"];

/// An axis-aligned region in a native Bedrock dimension. Cells are ordered x, z, y.
#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    pub dimension: i32,
    pub min: [i32; 3],
    pub size: [u8; 3],
    /// Explicitly permits loading and ticking destination chunks.
    pub load: bool,
}

impl Query {
    /// Checks transport bounds without claiming that every cell is available in the server.
    pub fn valid(&self) -> bool {
        usize::try_from(self.dimension).is_ok_and(|dimension| dimension < DIMENSIONS.len())
            && self
                .size
                .iter()
                .all(|extent| (1..=MAX_EXTENT).contains(extent))
            && self.min.iter().zip(self.size).all(|(min, size)| {
                min.checked_add(i32::from(size)).is_some()
                    && i64::from(*min).abs() <= MAX_COORDINATE
            })
    }

    pub fn cells(&self) -> usize {
        self.size.iter().map(|value| usize::from(*value)).product()
    }
}

/// A canonical named block permutation. Runtime IDs never cross server registries.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockState {
    pub name: String,
    pub states: String,
}

/// One bounded scan. A scan can span ticks; it is not an atomic world transaction.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Region {
    pub session: u64,
    pub revision: u64,
    pub palette: Vec<BlockState>,
    pub cells: Vec<u16>,
}

impl Region {
    pub fn valid(&self, query: &Query) -> bool {
        self.session != 0
            && query.valid()
            && self.cells.len() == query.cells()
            && self.palette.len() <= MAX_CELLS
            && self.palette.iter().all(|state| {
                !state.name.is_empty()
                    && state.name.len() <= MAX_NAME_BYTES
                    && state.states.len() <= MAX_STATE_BYTES
            })
            && self
                .palette
                .iter()
                .map(|state| state.name.len() + state.states.len())
                .sum::<usize>()
                <= MAX_METADATA_BYTES
            && self
                .cells
                .iter()
                .all(|cell| *cell == UNKNOWN || usize::from(*cell) < self.palette.len())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Unavailable,
    InvalidQuery,
    BudgetExhausted,
}

/// Nonblocking native source. The managed server owns sampling and subscription expiry.
pub trait Source: Send + Sync {
    fn read(&self, query: &Query) -> Result<Option<Arc<Region>>, Error>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_is_never_air_and_invalid_indices_are_rejected() {
        let query = Query {
            dimension: 1,
            min: [0, 64, 0],
            size: [1; 3],
            load: false,
        };
        let mut region = Region {
            session: 1,
            revision: 1,
            palette: vec![],
            cells: vec![UNKNOWN],
        };
        assert!(region.valid(&query));
        region.cells[0] = 0;
        assert!(!region.valid(&query));
        region.palette.push(BlockState {
            name: "minecraft:air".into(),
            states: "{}".into(),
        });
        assert!(region.valid(&query));
        assert!(
            !Query {
                size: [0; 3],
                ..query.clone()
            }
            .valid()
        );
        assert!(
            !Query {
                min: [i32::MAX; 3],
                ..query
            }
            .valid()
        );
    }
}
