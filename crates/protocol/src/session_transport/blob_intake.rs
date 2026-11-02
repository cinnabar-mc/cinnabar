use bedrock_protocol_1_26_60::packets::PacketId;
use std::mem::size_of;

use super::{Inbound, RawPacket, SessionError, SessionTransport};

/// The already inspected raw prefix, retained across cancelled body reads without rescanning it.
#[derive(Default)]
pub(super) struct BlobScan {
    cursor: usize,
    pub(super) bytes: usize,
    transfer_armed: bool,
}

impl BlobScan {
    /// Releases a scanned prefix item's raw-memory charge when ordinary FIFO intake consumes it.
    pub(super) fn consume_front(&mut self, packet: &RawPacket) {
        if self.cursor != 0 {
            self.cursor -= 1;
            self.bytes = self.bytes.saturating_sub(
                packet
                    .inner_frame()
                    .len()
                    .saturating_add(size_of::<RawPacket>()),
            );
        }
    }
}

impl<R: bridge::FrameRead> SessionTransport<R> {
    /// Reads only cache bodies past ordinary input, preserving every skipped packet in FIFO order.
    /// Returns None at the shared raw byte budget or a session boundary; normal intake then owns
    /// recovery. One whole frame may cross the bound, as with ordinary transport read-ahead.
    pub(crate) async fn recv_blob_body(
        &mut self,
        max_bytes: usize,
        transfer_armed: bool,
    ) -> Result<Option<Inbound>, SessionError> {
        self.current = None;
        if self.blob_scan.transfer_armed != transfer_armed {
            self.blob_scan = BlobScan {
                transfer_armed,
                ..Default::default()
            };
        }
        loop {
            while let Some(packet) = self.received.get_mut(self.blob_scan.cursor) {
                if packet.id == PacketId::ClientCacheMissResponse {
                    let packet = self
                        .received
                        .remove(self.blob_scan.cursor)
                        .expect("scanned packet");
                    self.observe(&packet);
                    return Ok(Some(Inbound::Packet(packet)));
                }
                if matches!(
                    packet.id,
                    PacketId::Transfer | PacketId::Disconnect | PacketId::ChangeDimension
                ) || (transfer_armed
                    && matches!(packet.id, PacketId::LevelChunk | PacketId::SubChunk))
                {
                    return Ok(None);
                }
                // A small retained update must not keep the body's whole batch allocation alive.
                *packet = packet.clone().into_retention_bounded();
                self.blob_scan.bytes = self.blob_scan.bytes.saturating_add(
                    packet
                        .inner_frame()
                        .len()
                        .saturating_add(size_of::<RawPacket>()),
                );
                self.blob_scan.cursor += 1;
            }
            if self.blob_scan.bytes.saturating_add(self.ahead_bytes) >= max_bytes
                || self.terminal.is_some()
            {
                return Ok(None);
            }
            self.receive_more().await?;
        }
    }
}

#[cfg(test)]
mod tests;
