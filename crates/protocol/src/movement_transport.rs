//! Sequenced movement admission identities and last-moment interaction invalidation.

use crate::Packet;
use simulation_clock::PhysicsTick;
use simulation_clock::identity::{PositionAuthorityGeneration, SessionGeneration};
use tokio::sync::watch;

/// Identity preserved through movement admission, acknowledgement and cancellation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicsSendIdentity {
    pub session_generation: SessionGeneration,
    pub tick: PhysicsTick,
    pub admission_id: u64,
    pub reanchor_epoch: PositionAuthorityGeneration,
}

/// An admitted interaction and its preceding actions, with a movement-only fallback.
#[derive(Debug)]
pub struct InteractionPacketGuard {
    authority_epoch: u64,
    authority: watch::Receiver<u64>,
    movement_packet: Packet,
    preceding_packets: Vec<Packet>,
}

impl InteractionPacketGuard {
    /// Freezes the interaction epoch together with its movement-only fallback.
    pub fn new(
        authority_epoch: u64,
        authority: watch::Receiver<u64>,
        movement_packet: Packet,
    ) -> Self {
        Self {
            authority_epoch,
            authority,
            movement_packet,
            preceding_packets: Vec::new(),
        }
    }

    /// Keeps a standalone action in the same admission and cancellation unit as its input.
    #[must_use]
    pub fn with_preceding_packet(mut self, packet: Packet) -> Self {
        self.preceding_packets.push(packet);
        self
    }

    /// Removes stale interactions and their preceding actions immediately before writing.
    pub fn sanitize(self, packet: Packet) -> (Vec<Packet>, Packet) {
        if *self.authority.borrow() == self.authority_epoch {
            (self.preceding_packets, packet)
        } else {
            (Vec::new(), self.movement_packet)
        }
    }
}

/// Admission failure for an atomic ordered packet batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchSendError {
    Full,
    Closed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requested_breaks_preserve_order_and_revoke_as_a_unit() {
        for revoke in [false, true] {
            let (epoch, receiver) = watch::channel(1);
            let packet = |x| crate::creative_destroy_block_packet(42, [x, 2, 3], 0);
            let guard = InteractionPacketGuard::new(1, receiver, packet(0))
                .with_preceding_packet(packet(1))
                .with_preceding_packet(packet(2));
            if revoke {
                epoch.send_replace(2);
            }
            let (preceding, movement) = guard.sanitize(packet(3));
            assert_eq!(preceding.len(), if revoke { 0 } else { 2 });
            for (index, actual) in preceding.iter().enumerate() {
                assert_eq!(
                    crate::encode(actual).unwrap(),
                    crate::encode(&packet(index as i32 + 1)).unwrap()
                );
            }
            assert_eq!(
                crate::encode(&movement).unwrap(),
                crate::encode(&packet(if revoke { 0 } else { 3 })).unwrap()
            );
        }
    }
}
