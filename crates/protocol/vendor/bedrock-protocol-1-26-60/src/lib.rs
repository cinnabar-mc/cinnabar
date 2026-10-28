// Code generated from canonical protocol manifest v2. DO NOT EDIT.

// Variant layouts follow the manifest; the emitter boxes by its own size estimate.
#![allow(clippy::large_enum_variant)]

pub const GAME_VERSION: &str = "1.26.60";
pub const PROTOCOL_VERSION: i32 = 2225;
/// Rust codec contract this crate implements; see protocolgen's
/// `docs/rust-codec-contract.md`.
pub const CODEC_CONTRACT: u32 = 2;
/// SHA-256 of the stable serialization of the manifest this crate was generated from.
pub const MANIFEST_SHA256: &str =
    "c1c90a44660b03e69fc84ff8f6dea3b0746bd29991c287dc184945921284204b";

pub mod enums;
pub mod fields;
pub mod packets;
pub mod types;
pub mod wire;
