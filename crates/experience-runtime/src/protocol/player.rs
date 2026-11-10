//! Protocol 5's player inventory and network scope, which a callback's snapshot may carry, the
//! stacks a guest stages, and protocol 12's containers in reach and the actor's game mode.

use serde::{Deserialize, Serialize};

use super::BlockPos;

/// An item stack; `data`, lowercase hex, is only on the Experience's own items, and `plain` is
/// set when the stack carries nothing else.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemStack {
    pub id: String,
    pub metadata: u16,
    pub count: u8,
    pub max_count: u8,
    pub data: Option<String>,
    pub plain: bool,
}

/// A stack the guest puts somewhere; `data` is lowercase hex.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewStack {
    pub id: String,
    pub metadata: u16,
    pub count: u8,
    pub data: Option<String>,
}

/// Slots of an inventory: 0 to 8 the hotbar, 9 to 35 the rest of the main inventory, 36 the
/// offhand.
pub const INVENTORY_SLOTS: usize = 37;
/// Slots of the hotbar, from which the selected slot is.
pub const HOTBAR_SLOTS: u8 = 9;

/// A registered variant with its stack limit, offhand eligibility and plain classification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerItem {
    pub id: String,
    pub metadata: u16,
    pub max_count: u8,
    pub off_hand: bool,
    pub plain: bool,
}

/// The actor's inventory: slots 0 to 8 the hotbar, 9 to 35 the rest of the main inventory, 36
/// the offhand; and the game mode it plays in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Inventory {
    pub selected: u8,
    pub slots: Vec<Option<ItemStack>>,
    pub game_mode: GameMode,
}

/// A player's game mode, as the adapter derives it from what the mode allows: `spectator`
/// without collision, `creative` with the creative inventory, `adventure` without editing the
/// world, and `survival` otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameMode {
    Survival,
    Creative,
    Adventure,
    Spectator,
}

/// The network around a callback's anchor: its loaded members, which the snapshot holds, and
/// whether its bounds cut it short.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Network {
    pub blocks: Vec<BlockPos>,
    pub truncated: bool,
}

/// The positions of a callback's container reach that hold a container or are not loaded, in the
/// order the adapter found them, and whether its bounds left some out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Containers {
    pub blocks: Vec<Container>,
    pub truncated: bool,
}

/// A container block, a vanilla one, with its slots in the container's own order, and `pair`,
/// the other half of a double chest, whose slots these are too; or, without `loaded`, a position
/// in an unloaded chunk, with neither.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Container {
    pub pos: BlockPos,
    pub loaded: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pair: Option<BlockPos>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slots: Vec<Option<ItemStack>>,
}
