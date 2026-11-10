//! `terrain.find-blocks` subscriptions, answered by the client from its loaded terrain.
use crate::{TerrainPosition, TerrainReadError};
use mod_api::{
    MAX_BLOCK_IDENTIFIER_BYTES, MAX_FOUND_BLOCK_IDENTIFIERS, MAX_FOUND_BLOCKS, MAX_TERRAIN_DISTANCE,
};

/// One admitted search: sorted, unique identifiers, a range in blocks from the feet and a
/// result limit.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockSearch {
    pub identifiers: Vec<String>,
    pub range: f32,
    pub limit: u32,
}

impl BlockSearch {
    /// Normalizes a guest request, rejecting names that are not namespaced block identifiers,
    /// a range outside 1 to `MAX_TERRAIN_DISTANCE` and a limit of zero or above
    /// `MAX_FOUND_BLOCKS`.
    pub fn admit(
        mut identifiers: Vec<String>,
        range: f32,
        limit: u32,
    ) -> Result<Self, TerrainReadError> {
        if !(1..=MAX_FOUND_BLOCK_IDENTIFIERS).contains(&identifiers.len())
            || !identifiers.iter().all(|name| identifier_valid(name))
            || !range.is_finite()
            || range < 1.0
            || limit == 0
        {
            return Err(TerrainReadError::InvalidQuery);
        }
        if range > MAX_TERRAIN_DISTANCE as f32 {
            return Err(TerrainReadError::OutOfRange);
        }
        if limit as usize > MAX_FOUND_BLOCKS {
            return Err(TerrainReadError::LimitExceeded);
        }
        identifiers.sort_unstable();
        identifiers.dedup();
        Ok(Self {
            identifiers,
            range,
            limit,
        })
    }
}

/// Loaded matches of one search, nearest first, in the world scope they were found in.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FoundBlocks {
    pub session: u64,
    pub dimension: i32,
    pub positions: Vec<TerrainPosition>,
}

/// The client's loaded terrain, as block searches see it.
pub trait BlockSearchSource {
    /// Answers one subscribed search with at most `search.limit` matches, nearest first. The
    /// source bounds its own per-frame work and may return results found so far.
    fn find(&mut self, search: &BlockSearch) -> Result<FoundBlocks, TerrainReadError>;
}

/// Answers every terrain-authorized host's searches before this frame's callbacks.
pub fn refresh_block_searches(
    hosts: &mut [&mut crate::ModHost],
    source: &mut impl BlockSearchSource,
) {
    for host in hosts
        .iter_mut()
        .filter(|host| host.is_active() && host.grants().terrain)
    {
        host.instance.refresh_block_searches(source);
    }
}

/// The latest answer to one subscription; none until the client first answers it.
pub(crate) type SearchSample = Option<Result<FoundBlocks, TerrainReadError>>;

/// Accepts `namespace:path` with lowercase ASCII, digits, `_`, `-`, `.` and a `/` in the path.
fn identifier_valid(name: &str) -> bool {
    if name.len() > MAX_BLOCK_IDENTIFIER_BYTES {
        return false;
    }
    let Some((namespace, path)) = name.split_once(':') else {
        return false;
    };
    let valid = |value: &str, slash: bool| {
        !value.is_empty()
            && value.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'_' | b'-' | b'.')
                    || (slash && byte == b'/')
            })
    };
    valid(namespace, false) && valid(path, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn admission_normalizes_names_and_types_each_bound() {
        let search = BlockSearch::admit(
            names(&[
                "minecraft:diamond_ore",
                "minecraft:ancient_debris",
                "minecraft:diamond_ore",
            ]),
            64.0,
            16,
        )
        .unwrap();
        assert_eq!(
            search.identifiers,
            names(&["minecraft:ancient_debris", "minecraft:diamond_ore"])
        );
        for (identifiers, range, limit, error) in [
            (names(&[]), 8.0, 1, TerrainReadError::InvalidQuery),
            (names(&["Diamond"]), 8.0, 1, TerrainReadError::InvalidQuery),
            (
                names(&["minecraft:a"]),
                f32::NAN,
                1,
                TerrainReadError::InvalidQuery,
            ),
            (
                names(&["minecraft:a"]),
                0.5,
                1,
                TerrainReadError::InvalidQuery,
            ),
            (
                names(&["minecraft:a"]),
                8.0,
                0,
                TerrainReadError::InvalidQuery,
            ),
            (
                names(&["minecraft:a"]),
                MAX_TERRAIN_DISTANCE as f32 + 1.0,
                1,
                TerrainReadError::OutOfRange,
            ),
            (
                names(&["minecraft:a"]),
                8.0,
                MAX_FOUND_BLOCKS as u32 + 1,
                TerrainReadError::LimitExceeded,
            ),
            (
                vec!["minecraft:a".to_owned(); MAX_FOUND_BLOCK_IDENTIFIERS + 1]
                    .into_iter()
                    .enumerate()
                    .map(|(index, name)| format!("{name}{index}"))
                    .collect(),
                8.0,
                1,
                TerrainReadError::InvalidQuery,
            ),
        ] {
            assert_eq!(BlockSearch::admit(identifiers, range, limit), Err(error));
        }
        assert!(
            BlockSearch::admit(
                names(&["minecraft:a"]),
                MAX_TERRAIN_DISTANCE as f32,
                MAX_FOUND_BLOCKS as u32
            )
            .is_ok()
        );
    }
}
