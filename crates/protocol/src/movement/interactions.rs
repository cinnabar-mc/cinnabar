//! Interaction payloads carried inside one current `PlayerAuthInput`.
//!
//! The start, abort, continue, and local prediction of
//! a destroy travel as the block-action list of the movement tick in which
//! they happened (`PerformBlockActions`). A creative instant destroy also uses
//! the standalone `PlayerAction` packet; its embedded break-block item-use
//! transaction (`PerformItemInteraction`) remains the client-authoritative
//! carrier. Block placement is a standalone transaction. This module owns
//! those payloads; the movement snapshot itself stays a pure movement
//! record.

use bedrock_protocol_1_26_60::enums::{ItemUseInventoryTransactionActionType, PlayerActionType};
use bedrock_protocol_1_26_60::types::{
    BlockPos, ItemStackLegacyRequestID, PackedItemUseLegacyInventoryTransaction,
    PlayerBlockActionData,
};
use bedrock_protocol_1_26_60::wire;
use thiserror::Error;

use crate::interaction::{BlockUsePacketError, BlockUseRequest, item_use_transaction};

/// The block-action kinds a client may place inside `PlayerAuthInput`.
///
/// Server-authoritative completion is [`Self::PredictDestroy`]; client-authoritative
/// completion is [`Self::StopDestroy`] beside an item-use destroy transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlockActionKind {
    /// Wire value 0: the player started destroying the block.
    StartDestroy,
    /// Wire value 1: the player stopped before the block was destroyed. Its
    /// face slot carries the whole-percent progress reached, `0..=100`.
    AbortDestroy,
    /// Wire value 2: a client-authoritative destroy completed.
    StopDestroy,
    /// Wire value 18: the destroy continues on the same block this tick.
    CrackBlock,
    /// Wire value 26: the client predicts that its destroy completed.
    PredictDestroy,
    /// Wire value 27: the destroy moved to a new block without a release.
    ContinueDestroy,
}

impl BlockActionKind {
    /// The pinned generated `PlayerActionType` value.
    #[must_use]
    pub fn wire_value(self) -> i32 {
        self.vendor().to_raw()
    }

    /// Maps a supported block action to the generated packet variant.
    const fn vendor(self) -> PlayerActionType {
        match self {
            Self::StartDestroy => PlayerActionType::StartDestroyBlock,
            Self::AbortDestroy => PlayerActionType::AbortDestroyBlock,
            Self::StopDestroy => PlayerActionType::StopDestroyBlock,
            Self::CrackBlock => PlayerActionType::CrackBlock,
            Self::PredictDestroy => PlayerActionType::PredictDestroyBlock,
            Self::ContinueDestroy => PlayerActionType::ContinueDestroyBlock,
        }
    }
}

/// One block action: the kind, the absolute block position, and the face
/// (`0` down, `1` up, `2` north, `3` south, `4` west, `5` east), except that an
/// abort carries its progress percent there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockAction {
    pub kind: BlockActionKind,
    pub position: [i32; 3],
    pub face: u8,
}

/// Order-preserving block actions accumulated for one movement tick.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BlockActions {
    actions: Vec<BlockAction>,
}

impl BlockActions {
    /// Starts an empty action list without allocating storage.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            actions: Vec::new(),
        }
    }

    /// Retains one action after all earlier actions from the same tick.
    pub fn push(&mut self, action: BlockAction) {
        self.actions.push(action);
    }

    /// Retains every action of `other` after this list's existing actions.
    pub fn append(&mut self, other: &Self) {
        self.actions.extend_from_slice(&other.actions);
    }

    /// Visits actions in the order they were accumulated.
    pub fn iter(&self) -> impl Iterator<Item = &BlockAction> {
        self.actions.iter()
    }

    /// Returns the number of retained actions.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.actions.len()
    }

    /// Returns whether this tick has any block actions.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }

    /// Discards retained actions while keeping their storage for reuse.
    pub fn clear(&mut self) {
        self.actions.clear();
    }

    /// Removes and returns every retained action.
    pub fn take(&mut self) -> Self {
        std::mem::take(self)
    }

    /// Validates each action and converts the complete list to wire values.
    pub(super) fn vendor(&self) -> Result<Vec<PlayerBlockActionData>, InteractionEncodeError> {
        self.iter()
            .map(|action| {
                let face_limit = match action.kind {
                    BlockActionKind::AbortDestroy => 100,
                    _ => 5,
                };
                if action.face > face_limit {
                    return Err(InteractionEncodeError::InvalidBlockActionFace(action.face));
                }
                let [x, y, z] = action.position;
                Ok(PlayerBlockActionData {
                    player_action_type: action.kind.vendor(),
                    position: BlockPos {
                        x: wire::ZigZag32(x),
                        y: wire::ZigZag32(y),
                        z: wire::ZigZag32(z),
                    },
                    facing: wire::ZigZag32(i32::from(action.face)),
                })
            })
            .collect()
    }
}

/// The one item-use transaction attached to a movement tick.
///
/// The client uses `Destroy`; `Use` stays encodable for wire fixtures.
#[derive(Debug, Clone, PartialEq)]
pub enum BlockItemInteraction {
    Use(BlockUseRequest),
    Destroy(BlockUseRequest),
}

/// Interaction payloads attached to one movement tick.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PlayerAuthInputInteractions {
    /// Destroy-family block actions in the order they happened.
    pub block_actions: BlockActions,
    /// At most one mutually exclusive block item-use transaction.
    pub block_interaction: Option<BlockItemInteraction>,
}

impl PlayerAuthInputInteractions {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.block_actions.is_empty() && self.block_interaction.is_none()
    }
}

/// Invalid interaction state that cannot be represented on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum InteractionEncodeError {
    #[error("block action face {0} is outside its kind's range")]
    InvalidBlockActionFace(u8),
    #[error("embedded break-block transaction is invalid: {0}")]
    InvalidBlockDestroy(#[from] BlockUsePacketError),
    #[error("embedded block-use transaction is invalid: {0}")]
    InvalidBlockUse(BlockUsePacketError),
    #[error(
        "PlayerAuthInput interaction flags were asserted without a matching payload (or the reverse)"
    )]
    InconsistentInteractionFlags,
}

pub(super) fn packed_block_interaction(
    interaction: BlockItemInteraction,
) -> Result<PackedItemUseLegacyInventoryTransaction, InteractionEncodeError> {
    let transaction = match interaction {
        BlockItemInteraction::Use(request) => {
            item_use_transaction(request, ItemUseInventoryTransactionActionType::Place)
                .map_err(InteractionEncodeError::InvalidBlockUse)?
        }
        BlockItemInteraction::Destroy(request) => {
            item_use_transaction(request, ItemUseInventoryTransactionActionType::Destroy)
                .map_err(InteractionEncodeError::InvalidBlockDestroy)?
        }
    };
    Ok(PackedItemUseLegacyInventoryTransaction {
        legacy_request_id: ItemStackLegacyRequestID(0),
        legacy_set_item_slots: None,
        item_use_transaction: transaction,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(kind: BlockActionKind, face: u8) -> BlockAction {
        BlockAction {
            kind,
            position: [1, -2, 3],
            face,
        }
    }

    #[test]
    fn wire_values_are_the_pinned_destroy_family() {
        for kind in [
            BlockActionKind::StartDestroy,
            BlockActionKind::AbortDestroy,
            BlockActionKind::StopDestroy,
            BlockActionKind::CrackBlock,
            BlockActionKind::PredictDestroy,
            BlockActionKind::ContinueDestroy,
        ] {
            assert_eq!(kind.wire_value(), kind.vendor().to_raw());
        }
    }

    #[test]
    fn list_preserves_order_when_taken() {
        let mut actions = BlockActions::new();
        for face in 0..6 {
            actions.push(action(BlockActionKind::CrackBlock, face));
        }
        let taken = actions.take();
        assert!(actions.is_empty());
        assert_eq!(taken.len(), 6);
        assert_eq!(
            taken.iter().map(|action| action.face).collect::<Vec<_>>(),
            [0, 1, 2, 3, 4, 5]
        );
    }

    #[test]
    fn append_keeps_both_lists_in_order() {
        let mut left = BlockActions::new();
        let mut right = BlockActions::new();
        for _ in 0..6 {
            left.push(action(BlockActionKind::CrackBlock, 1));
            right.push(action(BlockActionKind::StartDestroy, 2));
        }
        let original = right.clone();
        left.append(&right);
        assert_eq!(left.len(), 12);
        assert_eq!(right, original);
        assert!(
            left.iter()
                .take(6)
                .all(|entry| entry.kind == BlockActionKind::CrackBlock)
        );
        assert_eq!(
            left.iter().skip(6).copied().collect::<Vec<_>>(),
            right.iter().copied().collect::<Vec<_>>()
        );
    }

    #[test]
    fn faces_outside_the_cube_fail_closed() {
        let mut actions = BlockActions::new();
        actions.push(action(BlockActionKind::StartDestroy, 6));
        assert_eq!(
            actions.vendor().unwrap_err(),
            InteractionEncodeError::InvalidBlockActionFace(6)
        );
    }

    #[test]
    fn abort_carries_progress_percent_in_the_face_slot() {
        let mut actions = BlockActions::new();
        actions.push(action(BlockActionKind::AbortDestroy, 100));
        assert_eq!(actions.vendor().unwrap()[0].facing.0, 100);
        actions.clear();
        actions.push(action(BlockActionKind::AbortDestroy, 101));
        assert_eq!(
            actions.vendor().unwrap_err(),
            InteractionEncodeError::InvalidBlockActionFace(101)
        );
    }
}
