# Packet mods

A packet mod is the one kind of mod with raw packet access. It is a trusted component that relays every session packet: each batch, in both directions, passes through it before reaching the server or the client. That trust is the point and the risk, so packet mods sit outside the personal-mod model — a player installs one deliberately, under the `packets` grant, knowing it can see and change everything the session sends.

One packet mod relays a session. There is no merging of several.

## The contract

A packet mod targets the `packet-mod` world and exports four functions:

| Export | When it runs | Returns |
| --- | --- | --- |
| `start` | Once, when a session begins: the upstream login's display name, XUID and UUID, the login client data as JSON, and the server's startup packets through StartGame as one batch body | The resource packs the mod adds to the session, or an error |
| `on_batch` | For every relayed batch, with its direction (`from-client` or `from-server`) | None forwards the batch unchanged; some body replaces it, and an empty body drops it |
| `tick` | Timed work, about once per client tick (50 ms) | An `outbound` pair of batch bodies to send now, one to the client and one to the server; an empty body sends nothing |
| `stop` | When the session ends | Nothing |

A batch body is the session contract's framing: each packet as a varuint32 byte length followed by the encoded packet, header included, in the session's protocol. The mod works on whole batches under that framing; decoding individual packets is its own business.

Resource packs returned from `start` are zip archives whose manifest identity must not repeat another pack in the stack, and each declares whether it applies above the server's packs or below them.

## Building one

Build the guest for `wasm32-unknown-unknown` as in [Getting started](getting-started.md), then pack it for the packet-mod world:

```sh
cargo run -p mod-host --bin mod-host --locked -- pack --world packet-mod \
  target/wasm32-unknown-unknown/debug/my_packet_mod.wasm /tmp/my-packet-mod.wasm
```

The host accepts two binary forms: a `packet-mod` component, or a WASI preview 1 reactor core module exporting the world's canonical ABI. In Rust, build against `mod_api::packet_mod` bindings and export the world.

## Limits

Packet mods run on wall-clock deadlines instead of the per-frame fuel budget other mods use, because a relay cannot pause the session while it thinks.

| Limit | Value | Constant |
| --- | --- | --- |
| Component or module size | 256 MiB | `MAX_PACKET_MOD_BYTES` |
| Each linear memory | 1 GiB | `PACKET_MOD_MEMORY_BYTES` |
| Returned data per call | 256 MiB | `PACKET_MOD_TRANSFER_BYTES` |
| Instantiation deadline | 60 s | `PACKET_MOD_LOAD_DEADLINE` |
| `start` deadline | 5 s | `PACKET_MOD_START_DEADLINE` |
| Ordinary `on_batch`, `tick`, `stop` deadline | 250 ms | `PACKET_MOD_CALL_DEADLINE` |

Each session instantiates the mod afresh under the load deadline. Startup server batches delivered through `on_batch` use the 5-second start deadline too.

WASI is linked only with the `wasi` grant, and only clocks, randomness and captured output: no files, environment, arguments or network. Without the grant, WASI imports are not linked; a component that requires them fails to load.

## Failure means pass-through

Any trap, guest error or missed deadline quarantines the instance. A guest error returned from `start`, `on_batch` or `tick` does the same. From that moment every call passes its batch through unchanged and the mod is not called again for that session — the session survives the mod, not the other way round. The quarantine is logged with the first characters of the guest's error.

Write accordingly. Handle malformed batches by forwarding them, not by returning an error, unless you intend to end your mod's part in the session. Keep per-batch work well inside 250 ms; the deadline is a wall clock, so a slow machine is not an excuse the host accepts. And remember that `start` sees the whole startup sequence at once: the 5-second budget is sized for reading it, not for work you could defer to `tick`.

See the [packet-mod world](../reference/worlds.md#packet-mod) for the exact export signatures and the linked record fields.
