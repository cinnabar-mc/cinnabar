// Adapted from Jolyne (Axolotl Stack, https://github.com/axolotl-stack/axolotl-stack),
// Copyright (c) 2025 Jeremy Ianne, MIT License; see crates/protocol/vendor/LICENSE.

//! Packet entries with only their header parsed, so play can route by ID and decode bodies lazily.

use bedrock_protocol_1_26_60::packets::{self, PacketId};
use bedrock_protocol_1_26_60::wire;
use bytes::Bytes;

use crate::codec::SessionFailure;
use crate::packet::GameHeader;
use crate::packet_tap::PacketRecordId;
use crate::{Packet, SessionError};

/// One batch entry, `[length][header][body]`, whose body stays a slice of the received batch.
#[derive(Debug, Clone)]
pub(crate) struct RawPacket {
    pub(crate) header: GameHeader,
    pub(crate) id: PacketId,
    body: Bytes,
    inner_frame: Bytes,
    backing_len: usize, // bytes of the batch allocation this entry shares
    /// The packet tap's handle, set when the transport splits the entry from its batch.
    pub(crate) record: Option<PacketRecordId>,
    /// The core batch frame and slot it was split from, for the latency trace; startup is frame 0.
    pub(crate) trace_position: (u64, u32),
}

impl RawPacket {
    /// Decodes the whole entry under the server limits, rejecting a body the packet does not
    /// consume.
    pub(crate) fn decode(self) -> Result<Packet, SessionError> {
        self.decode_parts()
            .map(|(header, data)| Packet::from_parts(header, data))
    }

    /// Decodes like [`Self::decode`] but returns the header and payload by value, so a caller
    /// that consumes the payload at once skips the [`Packet`] box.
    pub(crate) fn decode_parts(self) -> Result<(GameHeader, packets::Packet), SessionError> {
        match crate::codec::decode_body(self.id, &self.body) {
            Ok(data) => Ok((self.header, data)),
            Err(wire::DecodeError::TrailingBytes(remaining)) => {
                Err(SessionFailure::PacketTrailingBytes {
                    packet_id: self.id,
                    body_len: self.body.len(),
                    remaining,
                }
                .into())
            }
            Err(source) => Err(SessionFailure::PacketDecode {
                packet_id: self.id,
                body_len: self.body.len(),
                body_preview: self.body.iter().take(32).copied().collect(),
                source,
            }
            .into()),
        }
    }

    /// The payload after the header varint.
    pub(crate) fn body(&self) -> &Bytes {
        &self.body
    }

    /// The complete entry, length prefix included.
    pub(crate) fn inner_frame(&self) -> &Bytes {
        &self.inner_frame
    }

    /// Copies the entry into an allocation of its own, so a queued packet cannot keep its
    /// whole batch resident.
    pub(crate) fn into_compact(self) -> Self {
        let body_start = self.inner_frame.len() - self.body.len();
        let inner_frame = Bytes::copy_from_slice(&self.inner_frame);
        Self {
            header: self.header,
            id: self.id,
            body: inner_frame.slice(body_start..),
            backing_len: inner_frame.len(),
            inner_frame,
            record: self.record,
            trace_position: self.trace_position,
        }
    }

    /// Detaches the entry from unrelated batch siblings, so retained slices are charged only for
    /// the bytes they keep alive.
    pub(crate) fn into_retention_bounded(self) -> Self {
        if self.backing_len > self.inner_frame.len() {
            self.into_compact()
        } else {
            self
        }
    }
}

/// A well-formed batch entry whose ID this protocol version does not define.
///
/// Vanilla's receive loop creates no packet for such an ID and reads on, so the session counts
/// and skips it instead of ending.
#[derive(Debug, Clone)]
pub(crate) struct UnknownPacket {
    pub(crate) id: u32,
    pub(crate) body: Bytes,
}

/// A validated batch that creates packet metadata only as its entries are consumed.
#[derive(Debug)]
pub(crate) struct RawBatch {
    frames: wire::Frames,
    backing_len: usize,
    remaining: usize,
}

impl RawBatch {
    /// Splits the next entry, or names the ID and body of one this protocol does not define.
    pub(crate) fn next_entry(&mut self) -> Option<Result<RawPacket, UnknownPacket>> {
        let frame = self.frames.next()?.expect("batch framing was validated");
        self.remaining -= 1;
        Some(raw_packet(frame, self.backing_len))
    }

    /// Entries not yet split, unknown IDs included.
    pub(crate) const fn remaining(&self) -> usize {
        self.remaining
    }
}

/// Yields the known packets in order, passing over entries with unknown IDs.
impl Iterator for RawBatch {
    type Item = RawPacket;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Ok(packet) = self.next_entry()? {
                return Some(packet);
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some(self.remaining))
    }
}

/// Validates every length-prefixed entry before yielding packets that share the batch bytes.
/// The byte bound applies without a packet-count cap or a per-entry metadata allocation. An
/// entry whose ID this protocol does not define is well-formed and does not fail the batch.
pub(crate) fn decode_batch_body(body: Bytes, max_bytes: usize) -> Result<RawBatch, SessionError> {
    if body.len() > max_bytes {
        return Err(SessionFailure::BatchTooLarge {
            actual: body.len(),
            max: max_bytes,
        }
        .into());
    }
    let backing_len = body.len();
    let mut offset = 0;
    let mut remaining = 0;
    for frame in wire::Frames::new(body.clone(), usize::MAX) {
        let frame = frame.map_err(|error| framing_failure(&body[offset..], error))?;
        offset += frame.frame().len();
        remaining += 1;
    }
    Ok(RawBatch {
        frames: wire::Frames::new(body, usize::MAX),
        backing_len,
        remaining,
    })
}

/// Decodes the first packet entry in `cursor`, whose ID must be one this protocol defines, and
/// advances past it.
#[cfg(test)]
pub(crate) fn decode_packet_raw(cursor: &mut Bytes) -> Result<RawPacket, SessionError> {
    decode_entry_raw(cursor).map(|entry| entry.expect("a packet ID this protocol defines"))
}

/// Splits the first entry in `cursor` like [`RawBatch::next_entry`] and advances past it.
#[cfg(test)]
pub(crate) fn decode_entry_raw(
    cursor: &mut Bytes,
) -> Result<Result<RawPacket, UnknownPacket>, SessionError> {
    let backing_len = cursor.len();
    let frame = wire::Frames::new(cursor.clone(), 1)
        .next()
        .unwrap_or(Err(wire::DecodeError::UnexpectedEof {
            needed: 1,
            remaining: 0,
        }))
        .map_err(|error| framing_failure(&cursor[..], error))?;
    let _ = cursor.split_to(frame.frame().len());
    Ok(raw_packet(frame, backing_len))
}

/// Splits a `0xfe`-marked uncompressed batch, as fixtures capture one, into raw packets.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn decode_batch(
    batch: Bytes,
    max_bytes: usize,
) -> Result<Vec<RawPacket>, crate::ProtocolError> {
    if batch.first() != Some(&wire::BATCH_HEADER) {
        return Err(crate::ProtocolError::InvalidBatchHeader {
            actual: batch.first().copied(),
        });
    }
    Ok(decode_batch_body(batch.slice(1..), max_bytes)?.collect())
}

/// Maps a framing failure of the entry starting at `rest`: a declared length past the input is
/// a truncated entry, while a cut-short or overlong length or header varint is malformed.
fn framing_failure(rest: &[u8], error: wire::DecodeError) -> SessionError {
    if let wire::DecodeError::TooManyFrames { limit } = error {
        return SessionFailure::TooManyPackets { max: limit }.into();
    }
    let mut reader = wire::Reader::new(rest);
    match reader.read_var_u32() {
        Ok(declared) if declared as usize > reader.remaining() => SessionFailure::TruncatedEntry {
            declared: declared as usize,
            available: reader.remaining(),
        },
        _ => SessionFailure::MalformedEntry(error),
    }
    .into()
}

/// Names a framed entry's packet, or returns the raw ID and body of one this protocol does not
/// define.
fn raw_packet(frame: wire::RawFrame, backing_len: usize) -> Result<RawPacket, UnknownPacket> {
    let header = frame.header();
    let Some(id) = PacketId::from_raw(header.id) else {
        return Err(UnknownPacket {
            id: header.id,
            body: frame.body(),
        });
    };
    Ok(RawPacket {
        header: GameHeader {
            id,
            from_subclient: u32::from(header.sender_subclient),
            to_subclient: u32::from(header.target_subclient),
        },
        id,
        body: frame.body(),
        inner_frame: frame.frame().clone(),
        backing_len,
        record: None,
        trace_position: (0, 0),
    })
}

#[cfg(test)]
mod tests;
