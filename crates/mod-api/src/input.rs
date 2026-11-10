//! Shared bounds and validation for physical input observations.

/// Maximum physical chords in one player binding snapshot.
pub const MAX_INPUT_BINDINGS: usize = 128;
/// Maximum physical chords copied by one binding read.
pub const MAX_INPUT_BINDINGS_PER_READ: usize = 16;
/// Maximum bytes in a stable action name in an input binding.
pub const MAX_INPUT_ACTION_BYTES: usize = 32;
/// Maximum physical keys in one controls sample or reservation.
pub const MAX_CONTROL_KEYS: usize = 64;
/// Maximum bytes in a physical key name supplied by local controls.
pub const MAX_CONTROL_KEY_BYTES: usize = 32;
/// Whether a physical key name fits the shared controls wire contract.
pub fn control_key_valid(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= MAX_CONTROL_KEY_BYTES
        && key.bytes().all(|c| c.is_ascii_alphanumeric())
}
