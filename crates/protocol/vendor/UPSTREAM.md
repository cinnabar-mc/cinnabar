# Pinned upstream sources

`bedrock-protocol-1-26-60/` is protocolgen's generated Rust contract crate for Bedrock 1.26.60
(protocol 2225), copied byte for byte with no local patches. Only `protocol` depends on it.
The target manifest's `codec_crate` names the vendor directory and Rust dependency alias;
`codec_package` pins the generated Cargo package name.

- Generator: https://github.com/cinnabar-mc/protocolgen
- Generator commit: `48cd9baad30cb7d78d1ca53a78b471b003cc9744` (`generated/1.26.60/rust-contract`)

The expected protocol, codec contract and manifest fingerprint are pinned once, in
`assets/bedrock-target.json`; a protocol test fails until a re-vendored crate matches them. The client reports the target game version to match gophertunnel.

The raw packet entries and spawn sequence (`src/raw_packet.rs`, `src/login/startup.rs`) are
adapted from Jolyne, the Axolotl Stack client session library.

- Axolotl Stack merge revision: `c4540512dc47833bb40363da7ad1161110d64b67`
- Retained license normalized SHA-256: `62c75fcb256604584191434b605dc3fe661d938a94b2c35836ef55011bf24184`

Jolyne is MIT licensed, Copyright (c) 2025 Jeremy Ianne; its license is `LICENSE` here, also
reproduced in `THIRD_PARTY_NOTICES.md`.

Content registries are maintained independently of the generated wire schema. The protocol crate
uses reviewed retail item and biome allowlists under `crates/protocol/data/`.
