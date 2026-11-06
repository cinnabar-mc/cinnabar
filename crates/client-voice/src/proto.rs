//! Wire protocol between voice clients and the room host's relay.
//!
//! Every datagram starts with `MAGIC` and a one-byte type. Integers are little endian.
//! Decoding never panics: malformed input yields a [`DecodeError`].

pub const MAGIC: u8 = 0xC7;
/// Largest encoded audio frame carried in one datagram.
pub const MAX_PAYLOAD: usize = 1200;

const T_HELLO: u8 = 1;
const T_WELCOME: u8 = 2;
const T_DENIED: u8 = 3;
const T_VOICE: u8 = 4;
const T_RELAYED: u8 = 5;
const T_KEEPALIVE: u8 = 6;
const T_BYE: u8 = 7;

/// A point in the world. `dim` is the Bedrock dimension id (0 overworld, 1 nether, 2 end).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Position {
    pub dim: u8,
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Position {
    pub fn new(dim: u8, x: f32, y: f32, z: f32) -> Self {
        Self { dim, x, y, z }
    }

    pub fn distance(&self, other: &Position) -> f32 {
        let (dx, dy, dz) = (self.x - other.x, self.y - other.y, self.z - other.z);
        (dx * dx + dy * dy + dz * dz).sqrt()
    }

    fn is_finite(&self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Packet {
    /// Client -> host: join the room with the shared secret.
    Hello { secret: u64, player_id: u64 },
    /// Host -> client: accepted; `range` is the maximum hearing distance in blocks.
    Welcome { range: f32 },
    /// Host -> client: wrong secret or room full.
    Denied,
    /// Client -> host: one encoded audio frame and the speaker's position.
    Voice {
        seq: u32,
        pos: Position,
        payload: Vec<u8>,
    },
    /// Host -> client: a frame from `speaker`, already filtered by distance.
    Relayed {
        speaker: u64,
        seq: u32,
        pos: Position,
        payload: Vec<u8>,
    },
    /// Client -> host: position update while silent, also keeps the session alive.
    KeepAlive { pos: Position },
    /// Client -> host: leaving.
    Bye,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    Empty,
    BadMagic,
    UnknownType(u8),
    Truncated,
    TooLong,
    NonFinite,
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for DecodeError {}

struct Reader<'a> {
    data: &'a [u8],
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        if self.data.len() < n {
            return Err(DecodeError::Truncated);
        }
        let (head, tail) = self.data.split_at(n);
        self.data = tail;
        Ok(head)
    }
    fn u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32, DecodeError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64, DecodeError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn f32(&mut self) -> Result<f32, DecodeError> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn pos(&mut self) -> Result<Position, DecodeError> {
        let pos = Position {
            dim: self.u8()?,
            x: self.f32()?,
            y: self.f32()?,
            z: self.f32()?,
        };
        if pos.is_finite() {
            Ok(pos)
        } else {
            Err(DecodeError::NonFinite)
        }
    }
    fn rest_payload(&mut self) -> Result<Vec<u8>, DecodeError> {
        if self.data.is_empty() {
            return Err(DecodeError::Truncated);
        }
        if self.data.len() > MAX_PAYLOAD {
            return Err(DecodeError::TooLong);
        }
        Ok(std::mem::take(&mut self.data).to_vec())
    }
}

fn put_pos(out: &mut Vec<u8>, p: &Position) {
    out.push(p.dim);
    out.extend_from_slice(&p.x.to_le_bytes());
    out.extend_from_slice(&p.y.to_le_bytes());
    out.extend_from_slice(&p.z.to_le_bytes());
}

impl Packet {
    /// Serializes the packet. Fails only if the audio payload is empty or too large.
    pub fn encode(&self) -> Result<Vec<u8>, DecodeError> {
        let mut out = vec![MAGIC];
        match self {
            Packet::Hello { secret, player_id } => {
                out.push(T_HELLO);
                out.extend_from_slice(&secret.to_le_bytes());
                out.extend_from_slice(&player_id.to_le_bytes());
            }
            Packet::Welcome { range } => {
                out.push(T_WELCOME);
                out.extend_from_slice(&range.to_le_bytes());
            }
            Packet::Denied => out.push(T_DENIED),
            Packet::Voice { seq, pos, payload } => {
                check_payload(payload)?;
                out.push(T_VOICE);
                out.extend_from_slice(&seq.to_le_bytes());
                put_pos(&mut out, pos);
                out.extend_from_slice(payload);
            }
            Packet::Relayed {
                speaker,
                seq,
                pos,
                payload,
            } => {
                check_payload(payload)?;
                out.push(T_RELAYED);
                out.extend_from_slice(&speaker.to_le_bytes());
                out.extend_from_slice(&seq.to_le_bytes());
                put_pos(&mut out, pos);
                out.extend_from_slice(payload);
            }
            Packet::KeepAlive { pos } => {
                out.push(T_KEEPALIVE);
                put_pos(&mut out, pos);
            }
            Packet::Bye => out.push(T_BYE),
        }
        Ok(out)
    }

    pub fn decode(data: &[u8]) -> Result<Packet, DecodeError> {
        let mut r = Reader { data };
        if data.is_empty() {
            return Err(DecodeError::Empty);
        }
        if r.u8()? != MAGIC {
            return Err(DecodeError::BadMagic);
        }
        let kind = r.u8()?;
        let packet = match kind {
            T_HELLO => Packet::Hello {
                secret: r.u64()?,
                player_id: r.u64()?,
            },
            T_WELCOME => {
                let range = r.f32()?;
                if !range.is_finite() {
                    return Err(DecodeError::NonFinite);
                }
                Packet::Welcome { range }
            }
            T_DENIED => Packet::Denied,
            T_VOICE => Packet::Voice {
                seq: r.u32()?,
                pos: r.pos()?,
                payload: r.rest_payload()?,
            },
            T_RELAYED => Packet::Relayed {
                speaker: r.u64()?,
                seq: r.u32()?,
                pos: r.pos()?,
                payload: r.rest_payload()?,
            },
            T_KEEPALIVE => Packet::KeepAlive { pos: r.pos()? },
            T_BYE => Packet::Bye,
            other => return Err(DecodeError::UnknownType(other)),
        };
        Ok(packet)
    }
}

fn check_payload(payload: &[u8]) -> Result<(), DecodeError> {
    if payload.is_empty() {
        Err(DecodeError::Truncated)
    } else if payload.len() > MAX_PAYLOAD {
        Err(DecodeError::TooLong)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos() -> Position {
        Position::new(1, 10.5, 64.0, -3.25)
    }

    #[test]
    fn every_packet_round_trips() {
        let packets = [
            Packet::Hello {
                secret: 0xDEAD_BEEF_0123_4567,
                player_id: 2535400000000001,
            },
            Packet::Welcome { range: 48.0 },
            Packet::Denied,
            Packet::Voice {
                seq: u32::MAX,
                pos: pos(),
                payload: vec![1, 2, 3],
            },
            Packet::Relayed {
                speaker: 7,
                seq: 9,
                pos: pos(),
                payload: vec![9; 100],
            },
            Packet::KeepAlive { pos: pos() },
            Packet::Bye,
        ];
        for p in packets {
            let bytes = p.encode().unwrap();
            assert_eq!(Packet::decode(&bytes).unwrap(), p);
        }
    }

    #[test]
    fn rejects_malformed_input_without_panicking() {
        assert_eq!(Packet::decode(&[]), Err(DecodeError::Empty));
        assert_eq!(Packet::decode(&[0x00, 1]), Err(DecodeError::BadMagic));
        assert_eq!(
            Packet::decode(&[MAGIC, 99]),
            Err(DecodeError::UnknownType(99))
        );
        assert_eq!(Packet::decode(&[MAGIC]), Err(DecodeError::Truncated));
        let full = Packet::Voice {
            seq: 1,
            pos: pos(),
            payload: vec![5; 10],
        }
        .encode()
        .unwrap();
        // Header: magic, type, seq (4), dim (1), x/y/z (12) = 19 bytes, then the audio.
        // Cutting inside the header, or leaving no audio at all, must be rejected.
        for cut in 0..=19 {
            assert!(Packet::decode(&full[..cut]).is_err(), "cut at {cut}");
        }
    }

    #[test]
    fn rejects_non_finite_positions() {
        let bad = Packet::KeepAlive {
            pos: Position::new(0, f32::NAN, 0.0, 0.0),
        };
        let bytes = bad.encode().unwrap();
        assert_eq!(Packet::decode(&bytes), Err(DecodeError::NonFinite));
    }

    #[test]
    fn rejects_empty_and_oversized_payloads() {
        let empty = Packet::Voice {
            seq: 0,
            pos: pos(),
            payload: vec![],
        };
        assert!(empty.encode().is_err());
        let big = Packet::Voice {
            seq: 0,
            pos: pos(),
            payload: vec![0; MAX_PAYLOAD + 1],
        };
        assert_eq!(big.encode(), Err(DecodeError::TooLong));
    }
}
