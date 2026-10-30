# Menu service feeds

Cinnabar's native menu reads account and service data through Rust workers in
[`launcher-host`](../crates/launcher-host/src/launcher_account.rs). The frame consumes published
snapshots through [`AccountControl`](../crates/launcher-host/src/account_control.rs); network and
disk I/O stay on the workers. Service clients and authentication belong to
[bedrock-services](https://github.com/cinnabar-mc/bedrock-services).

| Menu surface | Owner and data |
| --- | --- |
| Account menu | Microsoft device-code sign-in, saved-account validation, switching and sign-out. A pending sign-in commits only after verification. |
| Featured servers | The selected account's complete paginated Minecraft catalogue, projected into cards and original Gathering selections. |
| Selected experience | Player-count subscriptions and an explicit Gathering allocation when Play is activated. Display metadata never supplies an allocated route. |
| Home and inbox | Minecraft messaging, inbox counts, treatments, persona artwork and the supported Home cards. Message interactions use the original service session. |
| Realms | Account-bound world discovery, invitation preview/acceptance and explicit join resolution. |
| Friends | Xbox people and world discovery, retaining the selected membership and signaling owner. |
| Profile | Xbox/Minecraft profile data on an independent worker, refreshed by opening, retrying or changing accounts. |
| Server status | Anonymous NetherNet HTTP status, with bounded requests and a whole-attempt deadline. The response supplies population and message text; elapsed time supplies the displayed latency. |

The canonical featured list comes from the current account's service response. A successful read
publishes the complete result atomically. Cards that need a Gathering allocation keep their
opaque selection beside their presentation data, so activating a refreshed card cannot reuse
another account's join authority. Counts are requested only while their details are visible.

A direct server uses HTTP-signaled NetherNet. Service joins use their actual returned route:
the default address selects HTTP signaling, while legacy WebSocket and Player Messaging
recipients retain their respective NetherNet owners. Unknown service protocols are refused.
Service identifiers are never sent to the anonymous server-status path as hostnames.

Account changes and cancellation retire the workers' original generations. Refresh rotation and
derived service checkpoints share the account-cache leases. A failed authenticated request does
not silently become an offline session. Artwork is fetched and cached by the native host, then
published for the menu to draw.

The launcher draws its out-of-game screens with OreUI. In-game server screens and their pack
bindings remain owned by JSON-UI. There is no separate Go account service or menu JSON-RPC
endpoint; see [control interfaces](control-channel.md) for the developer and browser surfaces.
