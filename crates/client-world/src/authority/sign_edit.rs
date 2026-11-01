//! The server's requests to open a sign or command block editor, held until the presentation
//! takes them.

use protocol::{OpenCommandBlockEvent, OpenSignEvent};

use super::WorldAuthority;

/// One sign face the server asked the player to edit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SignEditRequest {
    pub position: [i32; 3],
    pub front: bool,
}

impl WorldAuthority {
    /// Retains the latest current-dimension sign request at its ordered commit position.
    pub fn consume_open_sign(&mut self, event: OpenSignEvent) {
        if event.dimension != self.current_dimension {
            return;
        }
        // A newer request supersedes an untaken one, as a second open would.
        self.pending_sign_edit = Some(SignEditRequest {
            position: event.position,
            front: event.front,
        });
    }

    /// Takes the pending sign-edit request, if any.
    pub fn take_pending_sign_edit(&mut self) -> Option<SignEditRequest> {
        self.pending_sign_edit.take()
    }

    /// Retains the latest current-dimension command block request at its ordered commit position.
    pub fn consume_open_command_block(&mut self, event: OpenCommandBlockEvent) {
        if event.dimension == self.current_dimension {
            self.pending_command_block = Some(event.position);
        }
    }

    /// Takes the position of the pending command block request, if any.
    pub fn take_pending_command_block(&mut self) -> Option<[i32; 3]> {
        self.pending_command_block.take()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use assets::RuntimeAssets;
    use protocol::WorldBootstrap;

    use super::*;

    /// Sign requests retain the latest matching dimension and are consumed once.
    #[test]
    fn latest_matching_sign_request_survives_reset_until_taken() {
        let assets = Arc::new(RuntimeAssets::diagnostic());
        let mut authority = WorldAuthority::new(
            WorldBootstrap {
                local_player_unique_id: 1,
                dimension: 0,
                local_player_runtime_id: 1,
                player_position: [0.0; 3],
                world_spawn_position: [0; 3],
                // This asset-free sign fixture never decodes terrain.
                air_network_id: 0,
                block_network_ids_are_hashes: false,
            },
            assets,
            None,
            [0.0; 3],
            None,
        );
        assert_eq!(authority.take_pending_sign_edit(), None);
        authority.consume_open_sign(OpenSignEvent {
            dimension: 0,
            position: [1, 2, 3],
            front: true,
        });
        authority.consume_open_sign(OpenSignEvent {
            dimension: 0,
            position: [4, 5, 6],
            front: false,
        });
        authority.consume_open_sign(OpenSignEvent {
            dimension: 1,
            position: [7, 8, 9],
            front: true,
        });
        authority.reset_dimension(4, 1);
        assert_eq!(
            authority.take_pending_sign_edit(),
            Some(SignEditRequest {
                position: [4, 5, 6],
                front: false,
            })
        );
        assert_eq!(authority.take_pending_sign_edit(), None);

        authority.consume_open_command_block(OpenCommandBlockEvent {
            dimension: 1,
            position: [1, 2, 3],
        });
        authority.consume_open_command_block(OpenCommandBlockEvent {
            dimension: 0,
            position: [7, 8, 9],
        });
        assert_eq!(authority.take_pending_command_block(), Some([1, 2, 3]));
        assert_eq!(authority.take_pending_command_block(), None);
    }

    /// The coordinates of a request left untaken name another block after a transition.
    #[test]
    fn a_dimension_transition_discards_a_pending_command_block_request() {
        let assets = Arc::new(RuntimeAssets::diagnostic());
        let mut authority = WorldAuthority::new(
            WorldBootstrap {
                local_player_unique_id: 1,
                dimension: 0,
                local_player_runtime_id: 1,
                player_position: [0.0; 3],
                world_spawn_position: [0; 3],
                air_network_id: 0,
                block_network_ids_are_hashes: false,
            },
            assets,
            None,
            [0.0; 3],
            None,
        );
        authority.consume_open_command_block(OpenCommandBlockEvent {
            dimension: 0,
            position: [1, 2, 3],
        });
        authority.reset_dimension(4, 1);
        assert_eq!(authority.take_pending_command_block(), None);
        // A request in the new dimension is kept.
        authority.consume_open_command_block(OpenCommandBlockEvent {
            dimension: 1,
            position: [4, 5, 6],
        });
        assert_eq!(authority.take_pending_command_block(), Some([4, 5, 6]));
    }
}
