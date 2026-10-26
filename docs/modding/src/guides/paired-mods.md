# Paired mods

A paired mod is one Rust project with a server guest and a client part, shipped in one `.cmod`. The example is Cosmic, in `examples/mods/cosmic`: it registers Moon rock and Moon key, sends a ping to the server, and draws a moon label when the reply arrives.

Paired mods use a different API from personal mods. The client part is built with the Experience SDK's `client` feature against the `server-bundle` world, not with `mod-api`; the server operator offers the signed client bundle to players. Its declared permissions are subject to the client-code consent flow and host policy, rather than the personal-mod controls in Settings → Mods. For the personal-mod model, see [Getting started](getting-started.md).

## Install the tools

Use a Cinnabar source checkout with its build prerequisites. Install Rust, Go and Make, then from the checkout root:

```sh
rustup target add wasm32-unknown-unknown
cargo install --locked --path tools/cxb
```

Keep the checkout: `cinnabar-cxb` uses its SDK, local server and client build tools.

## Create from Cosmic

```sh
cinnabar-cxb mod new cosmic
cp -R examples/mods/cosmic/src/. cosmic/src/
cp -R examples/mods/cosmic/assets/. cosmic/assets/
```

| File | Edit here |
| --- | --- |
| `src/shared.rs` | Block and item declarations and message fields |
| `src/server.rs` | Server callbacks and replies |
| `src/client.rs` | Client initialisation and drawing |
| `assets/moon.png` | Original block texture and item icon |

## Declare content and messages

Declarations live in `src/shared.rs` and drive code generation for both halves. This declaration registers the cube:

```rust
#[block(name = "Moon rock", texture = assets::ASSET_MOON_PNG, hardness = 2.0)]
pub const MOON_ROCK: BlockId = id!("cosmic:moon_rock");
```

The project line at the top declares the mod id and the client part's permissions:

```rust
#![project(id = "cosmic", permissions(messaging, ui))]
```

Declare requests with `#[message(..., to_server)]` and replies with `#[message(..., to_client)]`. Add `control` to a to-client message, as Cosmic's `Pong` does, when a reply must wait for transport credit instead of being dropped as best-effort. Give each message handler an `#[on_message]` method inside the entry impl; its last argument is the received message type. Clients send with `client::send(&Ping { request: 1 })` and servers reply with `ctx.send(&player, &Pong { request: ping.request })`. Direction mistakes fail compilation.

Cosmic's two halves, as a pair:

```rust
// src/server.rs
#[server]
impl Experience for Server {
    #[on_message]
    fn ping(ctx: &Callback, player: PlayerId, ping: Ping) -> Result<(), GuestError> {
        ctx.send(&player, &Pong { request: ping.request })
    }
}

// src/client.rs
#[client]
impl ClientPart for Client {
    fn registry() -> Option<&'static Registry> {
        Some(&crate::shared::REGISTRY)
    }

    fn init() {
        client::send(&Ping { request: 1 }).expect("send moon ping");
    }

    #[on_message]
    fn pong(pong: Pong) {
        ui::set_widget("cosmic.moon", &format!("☾ Cosmic reply {}", pong.request))
            .expect("draw moon label");
    }
}
```

From `examples/mods/cosmic/src/server.rs` and `src/client.rs`, with imports trimmed. The client part's authored initialisation and callbacks wait for the server's matching registry fingerprint; a mismatch stops the part and asks for a rebuild, so shared definitions and the two halves cannot drift apart silently. Treat that as a build signal, not player authorisation: always validate requests against server state, because a matching registry does not authorise the player who sent the message.

Field bounds are part of the declaration. Integer scalars take optional `min`/`max`; strings and typed ids require `#[field(max_bytes = N)]` counting UTF-8 bytes; a `Vec<T>` of a supported scalar requires `#[field(max_items = N)]`, plus `max_bytes` for text or typed-id elements. Booleans are supported too; floating-point and unsupported field types fail generation.

Assets under `assets/server/` go only to the server artifact and `assets/client/` only to the client bundle; anything else ships in both, which is where block textures and item icons belong. Every file generates a compile-checked constant such as `assets::ASSET_MOON_PNG`.

## Build and run

```sh
cinnabar-cxb mod build --project cosmic
cinnabar-cxb mod run --project cosmic
```

Build writes `cosmic/dist/cosmic.cmod`: the server WASM with its manifest and file index, the signed client bundle, the generated resource pack, and the shared registry with Go bindings for existing Go servers. Run rebuilds it, prepares the client through `make play-build`, starts an offline loopback server and joins a creative world. The first run also prepares the client's game carriers through the normal build. Accept the resource pack and the client-code consent prompt, then look for Moon rock and Moon key in creative inventory and `☾ Cosmic reply 1` below the running indicator.

Run keeps the world in `cosmic/.local/world` and client data in `cosmic/.local/client`; closing the client stops the server and its helpers. Unset `CARGO_TARGET_DIR`, `CARGO_BUILD_TARGET_DIR` and `CARGO_BUILD_TARGET` before running, because the development client and helpers share the checkout's target layout.

## Edit and reload

Change the label in `src/client.rs` or the texture, close the client, and rerun `cinnabar-cxb mod run`. That rebuilds both guests and replaces this mod's development installation. This command provides a restart loop, and shared-definition changes need both halves rebuilt. Keep existing block ids when reusing a saved world, or the world's saved identities will not match your registry.

## Package and hand over

```sh
cinnabar-cxb mod build --project cosmic --release
cinnabar-cxb mod install cosmic/dist/cosmic.cmod --server /path/to/world
```

Give the `.cmod` to the server operator. The first build creates `cosmic/.local/publisher.seed`, which signs the client bundle: keep it private and retain it for later builds. A different seed creates a different publisher identity, so existing publisher trust does not carry over. The server offers players only the signed client bundle and the ordinary resource pack; it does not send the server guest. Installation refuses an existing mod id — to replace one, stop the server and remove `<world>/mods/<id>` first.
