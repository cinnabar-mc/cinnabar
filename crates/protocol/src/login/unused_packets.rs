//! Validates unsupported packet surfaces before discarding them.

use super::{PacketId, PlaySession, ProtocolError, RawPacket};

/// Identifies unsupported packet surfaces whose payloads still require wire validation.
pub(super) fn is_unused(id: PacketId) -> bool {
    matches!(
        id,
        PacketId::ShowStoreOffer
            | PacketId::PurchaseReceipt
            | PacketId::ServerStoreInfo
            | PacketId::ClientboundMatchmakingState
            | PacketId::ServerboundMatchmakingCancel
            | PacketId::ClientboundStonecutterSetRecipe
            | PacketId::ServerboundStonecutterSetRecipe
            | PacketId::ServerboundCursorItemDrag
            | PacketId::SetPassengerOfBlock
            | PacketId::ServerboundRegisterAudioContent
            | PacketId::ClientboundPlayAudioContent
    )
}

impl<R: bridge::FrameRead> PlaySession<R> {
    /// Counts well-formed unused packets after decoding; malformed wire still ends the session.
    pub(super) fn discard_unused_packet(&mut self, raw: RawPacket) -> Result<(), ProtocolError> {
        let id = raw.id;
        if self.decode_world_candidate(raw)?.is_some() {
            self.unused_packets = self.unused_packets.saturating_add(1);
            tracing::debug!(?id, "skipping an unused packet");
        }
        Ok(())
    }

    /// Number of well-formed packets decoded and discarded because their feature is unavailable.
    pub fn unused_packet_count(&self) -> u64 {
        self.unused_packets
    }

    /// Number of well-formed server packets skipped because this protocol does not define their
    /// ID; vanilla's receive loop skips them the same way.
    pub fn unknown_packet_count(&self) -> u64 {
        self.transport.unknown_packet_count()
    }
}
