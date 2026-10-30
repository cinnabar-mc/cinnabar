# Upstream sources

`bedrock-protocol-1-26-60` is protocolgen's generated Rust contract crate for Bedrock 1.26.60
(protocol 2225), consumed from the exact Git dependency shared with bedrock-client.
Only `protocol` declares this codec directly in Cinnabar; generated source is not vendored.

- Generator: https://github.com/cinnabar-mc/protocolgen
- Generator commit: `d47f818fc6511ef622a4f37bada23e0cf14ca889` (`generated/1.26.60/rust-contract`)

The expected protocol, codec contract and manifest fingerprint are pinned in
`assets/bedrock-target.json`; a protocol test checks the resolved generated crate.
The client reports the target game version.

The raw packet entries and spawn sequence (`src/raw_packet.rs`, `src/login/startup.rs`) are
adapted from Jolyne, the Axolotl Stack client session library.

- Axolotl Stack merge revision: `c4540512dc47833bb40363da7ad1161110d64b67`
- Retained license normalized SHA-256: `62c75fcb256604584191434b605dc3fe661d938a94b2c35836ef55011bf24184`

Jolyne is MIT licensed, Copyright (c) 2025 Jeremy Ianne; its license is `LICENSE` here, also
reproduced in `THIRD_PARTY_NOTICES.md`.

Content registries are maintained independently of the generated wire schema. The protocol crate
uses reviewed retail item and biome allowlists under `crates/protocol/data/`.
