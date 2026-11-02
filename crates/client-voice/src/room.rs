//! Room codes: the host's address and the room secret packed into a short string that
//! players can paste, e.g. `K7QM-2XPA-...`. Joining needs nothing else.
//!
//! IPv4 only. Layout: 4 bytes address, 2 bytes port, 8 bytes secret, 1 byte checksum
//! (15 bytes = 24 base32 characters in six groups of four).

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

/// Base32 without easily confused characters (no 0/O, 1/I).
const ALPHABET: &[u8; 32] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoomCode {
    pub addr: SocketAddrV4,
    pub secret: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoomCodeError {
    WrongLength,
    BadCharacter,
    BadChecksum,
}

impl std::fmt::Display for RoomCodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let msg = match self {
            RoomCodeError::WrongLength => "the code has the wrong length",
            RoomCodeError::BadCharacter => "the code has an invalid character",
            RoomCodeError::BadChecksum => "the code has a typo",
        };
        f.write_str(msg)
    }
}

impl std::error::Error for RoomCodeError {}

fn checksum(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0x5A, |acc: u8, b| acc.rotate_left(1) ^ b)
}

impl RoomCode {
    pub fn new(addr: SocketAddrV4, secret: u64) -> Self {
        Self { addr, secret }
    }

    pub fn socket_addr(&self) -> SocketAddr {
        SocketAddr::V4(self.addr)
    }

    pub fn encode(&self) -> String {
        let mut bytes = Vec::with_capacity(15);
        bytes.extend_from_slice(&self.addr.ip().octets());
        bytes.extend_from_slice(&self.addr.port().to_be_bytes());
        bytes.extend_from_slice(&self.secret.to_be_bytes());
        bytes.push(checksum(&bytes));

        let mut chars = Vec::with_capacity(24);
        let (mut acc, mut bits) = (0u32, 0u32);
        for byte in bytes {
            acc = (acc << 8) | u32::from(byte);
            bits += 8;
            while bits >= 5 {
                bits -= 5;
                chars.push(ALPHABET[((acc >> bits) & 31) as usize]);
            }
        }
        debug_assert_eq!(bits, 0);
        chars
            .chunks(4)
            .map(|c| std::str::from_utf8(c).unwrap())
            .collect::<Vec<_>>()
            .join("-")
    }

    pub fn decode(text: &str) -> Result<Self, RoomCodeError> {
        let cleaned: Vec<u8> = text
            .bytes()
            .filter(|b| !b.is_ascii_whitespace() && *b != b'-')
            .map(|b| b.to_ascii_uppercase())
            .collect();
        if cleaned.len() != 24 {
            return Err(RoomCodeError::WrongLength);
        }
        let mut bytes = Vec::with_capacity(15);
        let (mut acc, mut bits) = (0u32, 0u32);
        for c in cleaned {
            let value = ALPHABET
                .iter()
                .position(|&a| a == c)
                .ok_or(RoomCodeError::BadCharacter)?;
            acc = (acc << 5) | value as u32;
            bits += 5;
            if bits >= 8 {
                bits -= 8;
                bytes.push(((acc >> bits) & 0xFF) as u8);
            }
        }
        let (body, check) = bytes.split_at(14);
        if checksum(body) != check[0] {
            return Err(RoomCodeError::BadChecksum);
        }
        let ip = Ipv4Addr::new(body[0], body[1], body[2], body[3]);
        let port = u16::from_be_bytes([body[4], body[5]]);
        let secret = u64::from_be_bytes(body[6..14].try_into().unwrap());
        Ok(Self {
            addr: SocketAddrV4::new(ip, port),
            secret,
        })
    }
}

/// Generates a room secret from per-process OS-seeded hasher keys.
///
/// This is fine for a casual friends' room. The client integration should replace it
/// with the OS CSPRNG if it ever hosts public rooms.
pub fn generate_secret() -> u64 {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let mut h = RandomState::new().build_hasher();
    h.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    );
    h.finish() ^ RandomState::new().build_hasher().finish().rotate_left(17)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> RoomCode {
        RoomCode::new(
            SocketAddrV4::new(Ipv4Addr::new(203, 0, 113, 7), 24454),
            0x0123_4567_89AB_CDEF,
        )
    }

    #[test]
    fn round_trips() {
        let code = sample();
        let text = code.encode();
        assert_eq!(text.len(), 6 * 4 + 5);
        assert_eq!(RoomCode::decode(&text).unwrap(), code);
    }

    #[test]
    fn tolerates_case_spaces_and_missing_dashes() {
        let text = sample().encode();
        let messy = text.to_lowercase().replace('-', "  ");
        assert_eq!(RoomCode::decode(&messy).unwrap(), sample());
    }

    #[test]
    fn detects_typos_and_bad_input() {
        let text = sample().encode();
        let mut chars: Vec<char> = text.chars().collect();
        chars[0] = if chars[0] == 'A' { 'B' } else { 'A' };
        let typo: String = chars.into_iter().collect();
        assert_eq!(RoomCode::decode(&typo), Err(RoomCodeError::BadChecksum));
        assert_eq!(RoomCode::decode("ABCD"), Err(RoomCodeError::WrongLength));
        assert_eq!(
            RoomCode::decode(&"0".repeat(24)),
            Err(RoomCodeError::BadCharacter)
        );
    }

    #[test]
    fn secrets_differ() {
        assert_ne!(generate_secret(), generate_secret());
    }
}
