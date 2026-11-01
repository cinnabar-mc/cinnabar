//! Presentation access to authoritative sign and command block edit requests.

use client_world::SignEditRequest;
use client_world::ingestion::OpenSignEvent;

use super::WorldStream;

impl WorldStream {
    /// Retains a sign request at its ordered commit position.
    pub(super) fn consume_open_sign(&mut self, event: OpenSignEvent) {
        self.authority.consume_open_sign(event);
    }

    /// Takes the pending sign-edit request, if any.
    pub fn take_pending_sign_edit(&mut self) -> Option<SignEditRequest> {
        self.authority.take_pending_sign_edit()
    }

    /// Takes the position of the pending command block request, if any.
    pub fn take_pending_command_block(&mut self) -> Option<[i32; 3]> {
        self.authority.take_pending_command_block()
    }
}
