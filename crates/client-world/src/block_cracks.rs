//! Shared observations of ordered block-crack progress.

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BlockCrackStatus {
    pub active: usize,
    pub server_value_sum: i64,
    pub consumed: u64,
    pub orphan_updates: u64,
    pub capacity_rejections: u64,
    pub unsupported_targets: u64,
    pub retired_targets: u64,
    pub completed: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ActiveBlockCrack {
    pub position: [i32; 3],
    pub start_sequence: u64,
    /// The server's speed in progress units per game tick.
    pub server_value: i32,
    /// Progress accumulated on rendered frames; complete entries are removed.
    pub progress: f32,
    pub layers: [Option<u32>; world::MAX_STORAGE_COUNT],
}

#[derive(Clone, Debug, PartialEq)]
pub struct BlockCrackSnapshot {
    pub session_id: u64,
    pub dimension: i32,
    /// Changes on every committed dimension replacement, including same-ID replacements.
    pub dimension_sequence: Option<u64>,
    pub entries: Vec<ActiveBlockCrack>,
    pub status: BlockCrackStatus,
}
