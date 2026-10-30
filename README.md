# Cinnabar

> An independent, unofficial client compatible with Minecraft: Bedrock Edition. Not approved by
> or associated with Mojang or Microsoft. Minecraft is a trademark of Microsoft Corporation.

> [!WARNING]
> Cinnabar is a non-vanilla client and still in development. Its movement and combat may not yet
> match the official client exactly, so server anticheats may flag you and some servers may ban
> you. Play on servers at your own risk.

A Minecraft: Bedrock Edition client written from scratch in Rust. It plays on real servers, Realms
and friends' worlds with vanilla behaviour, and it's built to run much faster than the official
client.

[![Discord](https://img.shields.io/badge/Discord-Join%20us-5865F2?logo=discord&logoColor=white)](https://discord.gg/MeEz7BEHcM)
[![Website](https://img.shields.io/badge/Website-cinnabar.gg-B22222)](https://cinnabar.gg/)

<img width="1282" height="752" alt="Cinnabar in game" src="https://github.com/user-attachments/assets/ca040799-e00e-4a6e-85f8-a0e28af6ea72" />

| | |
| --- | --- |
| <img alt="Sunset with a shader pack" src="https://github.com/user-attachments/assets/25a2e8fa-23d8-4aeb-8e80-aca9d09dcee3" /> | <img alt="Moonlit river with a shader pack" src="https://github.com/user-attachments/assets/c0a8cdd8-03f2-4188-ae3c-36f516260060" /> |
| <img alt="Light shafts in a cave with a shader pack" src="https://github.com/user-attachments/assets/1622104f-7ea2-4dde-a433-219a5f09103b" /> | <img alt="PvP on iOS" src="https://github.com/user-attachments/assets/b6135da0-ff6a-4b75-ade0-56439be56ca0" /> |

## Download

- **Stable:** [latest release](https://github.com/cinnabar-mc/cinnabar/releases/latest)
- **Nightly:** daily builds of `dev` for macOS, Windows and Linux ([nightly](https://github.com/cinnabar-mc/cinnabar/releases/tag/nightly))

On first launch, Cinnabar downloads the vanilla resource pack after you accept the Minecraft EULA.
No Mojang assets are bundled. The release notes explain how to open unsigned builds.

## Build and run

```sh
make play
```

This fetches and compiles the vanilla assets and opens the Rust client launcher. Sign-in
uses a Microsoft device code. Credentials remain in the private per-user account store; never
share or commit that store. `make play PROFILE=release` builds the fully optimised binary,
and `make help` lists every target.

On Debian/Ubuntu, install `libwayland-dev` and `libudev-dev` first.

## What's different

Vanilla behaviour is the default. Everything below is extra:

| Feature | What it does |
| --- | --- |
| **Performance** | Frame pacing, latency and chunk streaming built to beat vanilla. |
| **1.12 PvP feel** | Ships with Bedrock 1.12-style combat visuals: the classic attack swing, hurt flash, enchantment glint and view bob, plus Java-style first-person hand sway and nametags. |
| **Live resource packs** | Add, remove or reorder packs without leaving the world. |
| **Multi-account manager** | Sign in with several Microsoft accounts and switch between them from the launcher, no signing out. |
| **Custom skins** | Import custom-geometry (4D/5D) skins and skin packs. See [docs/custom-skins.md](docs/custom-skins.md). |
| **Discord** | Rich presence, plus joining and inviting friends through Discord. See [docs/discord.md](docs/discord.md). |
| **Cinnabar players** | A badge on other Cinnabar players in the tab list, and update notices as soon as a release ships. See [docs/cinnabar-api.md](docs/cinnabar-api.md). |
| **Mods** *(preview)* | Personal WebAssembly mods with reload and explicit grants, plus paired server/client mods with declared blocks, items and typed messages. Start with [the mod guide](docs/modding/src/appendix/paired-mods.md); server client-code execution currently requires the developer switch. |
| **JSON-UI editor** | Preview and edit pack UI exactly as Cinnabar renders it, at [jsonui.cinnabar.gg](https://jsonui.cinnabar.gg/). |
| **Figma to JSON-UI** | A Figma plugin that turns a frame into a JSON-UI screen and resource pack. [Download it](https://jsonui.cinnabar.gg/figma-plugin.zip), or see [tools/figma-plugin](tools/figma-plugin/README.md). |

## How it works

The Rust client owns rendering, input, gameplay, accounts and network sessions. Direct servers
use HTTP-signaled NetherNet: HTTP exchanges connection details, then WebRTC carries game traffic
over ICE, DTLS and SCTP. The target game version and registry inputs are pinned in
[`assets/bedrock-target.json`](assets/bedrock-target.json).

Launch the menu, sign in or select a saved account, then choose a server:

```sh
cargo run -p cinnabar
```

Or start with a direct destination:

```sh
cargo run -p cinnabar -- --address HOST:PORT
```

A bare address uses addressed NetherNet discovery. Explicit `http://` and `https://` signaling
URLs keep their selected scheme and port. `--native-networking nethernet-http` selects HTTP
signaling explicitly; `--native-networking nethernet-lan` selects directed IPv4 LAN discovery.
Release packages and the mobile hosts enable `native-networking`; builds without it cannot
open native sessions.

Microsoft sign-in and saved-account validation run in Rust through the existing account menu.
A pending sign-in commits only after identity verification, and account changes or cancellation
retire the original request. Joins retain the selected account and connection key through Login,
resource packs, startup, retries and transfers. Direct servers that permit offline play can use
an offline identity when no account is selected.

Featured experiences, Realms and friend worlds retain their original account-bound selections.
The selected service response chooses the route; legacy WebSocket and Player Messaging signaling
remain available for service-owned NetherNet worlds. A raw `friend_xuid/<xuid>` string does not
supply a verified world selection. See [menu services](docs/menus-services.md) and
[control interfaces](docs/control-channel.md).

Local worlds run on the independent Dragonfly server or official BDS. Both join through their
owned loopback NetherNet route and process generation. Dragonfly remains a Go integration;
client accounts and networking use no separate Go process. See [local worlds](docs/local-worlds.md).
Server packs can come from CDN archives, the verified local cache or game-socket chunks.

The protocol cores are sans I/O. Socket and timer adapters are separate; Cinnabar drives the
native Tokio adapter on its application I/O runtime. NetherNet also provides a Bevy adapter for
caller-driven outbound sessions. Browser clients connect through the
[native gateway](web/gateway/README.md), which owns its upstream NetherNet connections.
Android includes native networking and platform verification; physical-device acceptance and
live friend/hosting coverage remain tracked in [plan.md](plan.md).

Built on these libraries:

| Library | Used for |
| --- | --- |
| [bedrock-net](https://github.com/cinnabar-mc/bedrock-net) | Batch framing, compression, Login, resource packs and typed client/server sessions. |
| [bedrock-services](https://github.com/cinnabar-mc/bedrock-services) | Microsoft/Xbox authentication, Minecraft credentials, Realms, profiles, presence and signaling. |
| [nethernet](https://github.com/cinnabar-mc/nethernet) | Sans-I/O ICE, DTLS and SCTP, with HTTP, LAN and service signaling adapters. |
| [protocolgen](https://github.com/cinnabar-mc/protocolgen) | The pinned generated Bedrock packet codec shared with bedrock-net. |
| [gophertunnel](https://github.com/Sandertv/gophertunnel) | Bedrock server sessions in the independent local-world server and Go fixture tools. |
| [go-raknet](https://github.com/Sandertv/go-raknet) | Retained compatibility dependency of the Go reference and fixture stack. |
| [dragonfly](https://github.com/df-mc/dragonfly) | The built-in local-world server in `tools/localserver`. |

The packet-entry and startup code in `crates/protocol` was adapted from Jolyne
([Axolotl Stack](https://github.com/axolotl-stack/axolotl-stack)); see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

Crate-by-crate layout: [docs/architecture/workspace.md](docs/architecture/workspace.md).
Benchmarks: [docs/benchmarks.md](docs/benchmarks.md).
Roadmap: [plan.md](plan.md). Change history: [CHANGELOG.md](CHANGELOG.md).

## Contributing

Open pull requests against `dev`, the default and release branch. Before pushing, check only what
your change affects:

```sh
cargo run -p devtool --locked -- verify-affected --base origin/dev
```

Contributor rules are in [AGENTS.md](AGENTS.md) and [docs/agents/](docs/agents/).

## Star history

[![Star history chart](https://api.star-history.com/svg?repos=cinnabar-mc/cinnabar&type=Date)](https://star-history.com/#cinnabar-mc/cinnabar&Date)

## License

[PolyForm Noncommercial 1.0.0](LICENSE). Contact the maintainers for a commercial license.
Third-party code and assets keep their own licenses; see
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
