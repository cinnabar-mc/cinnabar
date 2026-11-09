# Server Experience runtime

For a paired Rust mod, use [the author guide](modding/src/appendix/paired-mods.md). This reference is
for server callbacks, saved data and hosts of `experience-runtime`.

## Running and artifacts

`make local-server` builds `bedrock-local-server`, `experience-runtime` and the
bundle verifier. Installed `.cmod` packages are discovered under `<world>/mods/`.
For unpackaged server artifacts:

```sh
bedrock-local-server -dir <world> -addr 127.0.0.1:19132 \
  -experiences <artifacts> -experience-runtime <binary>
```

Each immediate artifact directory contains `experience.toml`, `files.toml`,
`server.wasm` and `assets/`. The manifest declares `id`, `version`, `api` and
`data-schema`; `[client]` is ignored by the server runtime, and the bundle builder
validates it separately. Other unknown keys are refused. The generated
`files.toml` maps every other file's path to its lowercase SHA-256, one
`"<path>" = "<sha256>"` line each, and loading refuses an unindexed, missing or
changed file. Paths are relative, slash-separated and cannot contain traversal or
symlinks. An artifact holds at most `MAX_ARTIFACT_ENTRIES` files and directories.
Artifacts built when the manifest held this index as `[files]` are refused; move
the table's entries to `files.toml`. See
[manifest validation](../crates/experience-runtime/src/manifest.rs).

The module must implement its declared server WIT. Client components, WASI,
unknown imports and serialized native Wasmtime artifacts fail loading. Hashes
prove integrity, not trust; install server artifacts as carefully as server code.
Helpers run as the server's OS user without an OS sandbox.

All artifacts load and register before listening. Duplicate IDs, a missing installed
Experience or a missing saved block definition fail startup. `minecraft` is reserved.
Do not edit an artifact during startup: texture registration follows manifest checking.

## Callbacks

Implement `experience_sdk::server::Experience`. A paired project's `#[server]`
entry point owns registration and registry replies; low-level guests can use
`export_experience!`. [SDK contracts](../crates/experience-sdk/src/server.rs) and
[server WIT](../crates/experience-sdk/wit/server/server.wit) define the types.

| Callback | When and scope |
| --- | --- |
| `registration` / `register` | Startup; declares blocks and items, or plain cubes only |
| `entity_definitions` | Startup; immutable actor identities, appearances, seats and properties |
| `on_entity` | Ordered interactions, attacks, seat changes, steering and despawn observations |
| `world_definitions` | Startup; immutable world definitions and optional startup instances |
| `on_player` | Ordered join, leave, world-change and sampled move observations |
| `on_place` | After player placement of an owned block |
| `on_break` | After player break; includes old ID and data |
| `on_interact` | Player uses an owned block; interaction is consumed immediately |
| `on_neighbor_changed` | A neighbor of an owned block changes |
| `client_message` | Actor's typed client record; valid focus snapshot or no block access |
| `epoch` | Client part changes world epoch; resend required state |
| `on_scheduled_update` | An owned block's scheduled update becomes due |
| `on_tick` | Each fixed tick of a bound world instance, with or without viewers |
| `on_timer` | A timer from `Callback::schedule` becomes due |

Block callbacks do not report changes from liquids, explosions or other plugins.
A fresh guest instance runs each server callback: memory does not survive it.
Use block data or scoped records for durable state. `callback-info.tick` supplies
the world tick; there is no ambient clock or randomness. A borrowed `Callback` expires at return.

## Reads, writes and commits

| Operation | Contract |
| --- | --- |
| Read block/state/data | Anchor, its six neighbors and network members, from a snapshot plus staged writes |
| `set_block` | Anchor/neighbor write scope in the anchor's chunk column; replaces air or owned blocks with air or owned blocks |
| `set_block_data`, `set_block_state` | Owned blocks in the write scope, plus snapshotted network members |
| `tell` | Event actor only; no control characters or formatting prefix |
| `send_client` | Actor's active client part on its declared channel; ordinary best-effort delivery after commit |
| `send_control` | Same recipient rules; retains negotiation/acknowledgment replies when transport credit is exhausted |
| `inventory`, `set_slot` | Actor's snapshot; writes are checked again at commit |
| `drop_item` | Fresh stack at a position where `set_block` may write |
| `crafting_result` | Matches the supplied grid against the host's crafting-table recipes |
| `schedule_update` | Owned block in data-write scope; survives restart, runs when loaded and still unchanged |

An accepted mutation is staged. A guest error or trap discards all gameplay output;
a refused individual operation stages nothing. Logs survive failures. The adapter
commits block/inventory results in one world transaction, after checking snapshot
tokens, written inventory slots and actor presence. Lifecycle operations run
outside that transaction. A stale result applies nothing.
Tells follow world mutations; client messages follow the world transaction.
A full block-event queue drops and counts new events without blocking the world
thread. A full player-observation queue suspends that Experience to avoid gaps.

Replacing a block, even with the same ID, clears its data and starts a new placement
generation. State changes retain data and generation. Missing data differs from empty data.
Unknown, unloaded and out-of-scope cells are not air; inspect returned errors.

Focus is the player's last interacted block for that Experience. It becomes invalid
on replacement, unload, dimension/world change or excessive distance. The current
distance uses the provisional `provisionalFocusRange` in the adapter's
[limits](../tools/localserver/experience/limits.go). It is incomplete parity work.

Network membership follows face-adjacent owned members. Break callbacks include
both sides of a split. Guests decide which faces actually connect. Flooding stops
at unloaded cells and network/data/frame budgets; treat a truncated network as
unavailable. Commit checks members and boundary changes.

## Worlds and player lifecycle

Server WIT 0.8 adds world definitions, player observations and scoped state.
The SDK defaults the new exports to empty/no-op implementations. Rebuilt guests
must declare the current server API (`api = "0.12"`); existing 0.1–0.6, 0.8, 0.9, 0.10 and 0.11
artifacts retain their frozen ABIs.

| Type or operation | Contract |
| --- | --- |
| `world_definitions` | Mod-namespaced immutable definitions; empty/flat generator, Overworld/Nether/End environment, persistent/temporary terrain |
| Definition, instance, handle | Distinct identities: a definition, stable storage ID, and one opening's generation; network dimension numbers stay in the host |
| Startup instances | Open before players join on every start; runtime-created instances reopen only on request |
| `Callback::world` | Current instance opening; legacy `info` metadata keeps its meaning |
| `resolve_world` | Snapshot lookup of this mod's open instances |
| `create_world` | Open an instance of an already registered definition |
| `unload_world` | Refuse occupied or reserved instances; invalidate handles before closing |
| `transfer` | Move only the callback actor to an authorized handle and finite position |
| `player_location` | Only the callback actor's live location; unavailable for leave |

Each lifecycle mutation must be the callback's only staged operation. Acceptance
means staged for host validation, not completed. Observe creation in a later
`resolve_world` and travel through `on_player`. Stale handles fail after unload,
reopen or server restart. World definitions cannot be added in callbacks.

Persistent terrain uses `<world>/experience-worlds/<mod>/<instance>/`. Reopening
retains terrain and private records but gives a new handle generation. Changing
a saved definition needs an explicit migration. Temporary terrain survives chunk
eviction, then disappears on close; its block records are removed and quota is
refunded. Fresh temporary openings also clear old block records after a crash.
Scoped records have independent lifetimes. Instances share the local gameplay
settings and pause state; new instances start at their generator's spawn, while
persistent reopenings retain saved spawn settings.

| `on_player` event | Contract |
| --- | --- |
| Join | Initial committed placement, before movement observations |
| World change | Historical before/after locations with handles and epochs |
| Move | First/last committed sampled positions; at most once per player per host tick, independent of pauseable world clocks |
| Leave | Historical location and private player-state access; no live inventory, position reads, messages or travel |

Join, move and world-change include inventory snapshots; writes recheck slots.
Historical locations remain readable after later travel; writes still need current
session/epoch authority. Departure clears focus and invalidates queued work, reads
and deferred sends, even after returning to the same live instance.

The host prepares the destination before detach. Failed admission restores the
source with a newer epoch, including cancellation after detach. A disconnected
player loses authority; cancellation after admission leaves the destination in
charge. Client traffic stays fenced until the correlated acknowledgment and any
preparation finish; see [wire fencing](server-experiences.md#versions-and-wire).
An active wire-v1 client part blocks coordinated departure; mandatory native moves
end that part. Initial placement can still activate v1. Ordinary players need no
extension notice. Failed notice delivery ends only the optional client part.
Acknowledgment does not prove terrain has rendered; live client-part transfer
qualification and crash-atomic multi-provider saves remain incomplete.

Player saves pair stable instance identity and position in `<world>/player-worlds/`.
Reconnect uses that instance's current opening. If closed or missing from an older
save, use the default Overworld spawn, without the other instance's coordinates.

## Ticks and timers

Server WIT 0.9 adds `on_tick` and `on_timer`; older artifacts receive neither.

| Rule | Contract |
| --- | --- |
| Clock | `SimulationStep` carries the counter and fixed tick duration (ns). Unload pauses it; reopening resumes with a new world generation. Process-local: not restored after a server restart |
| `Callback::schedule(after, period)` | Handle scoped to this mod and instance opening. Delay starts at commit; period 0 runs once, otherwise repeats. Unload discards the opening's timers |
| `cancel_schedule` | Cancels an active handle, including an occurrence already queued behind the caller |
| Final leave | No simulation context: scheduling and cancelling return `Unavailable`; private-state writes still work |
| Order | Per tick: `on_tick`, then due timers by handle, in the mod's FIFO. Callbacks see their admitted tick even if they run later |
| Overload | Queue overflow drops and counts ticks and repeating occurrences. A failed one-shot stays due. Repeating timers keep at most one queued occurrence; missed periods are skipped, never replayed |
| Limits | Active timers per mod and instance share `MAX_SCHEDULED_UPDATES`; delays use `MAX_SCHEDULE_TICKS` |

## Cell overlays

Server WIT 0.10 lets a block carry up to `MAX_CELL_OVERLAYS` (12) extra visuals in its own
cell, such as a cable bus's parts or a drive's cells. The new `overlays` export declares which
of the Experience's blocks are hosts and up to `MAX_OVERLAY_TYPES` (256) overlay types; 0.1–0.9
artifacts keep working without overlays.

| Rule | Contract |
| --- | --- |
| Overlay type | A look only overlays draw: states, visual and permutations checked as a block's, registered as a hidden block (no item, never placed). Its selection box, if any, picks it; without one it is never picked. Its collision is ignored |
| Overlay | `slot` 0–11, unique in the cell; drawing and picking go in slot order. `face` turns the type's model, authored with its front out of north and its top up, to that face (side faces keep their top up, up and down faces put it north): [`overlay_face.rs`](../crates/block-transform/src/overlay_face.rs). Permutation rotations apply before the face turn |
| Item overlay | `item` drawn flat on the face as an inventory slot shows it, at full brightness, `size` pixels wide (at most `MAX_OVERLAY_ITEM_SIZE`), centred `x`, `y` pixels right of and above the face's centre (at most `MAX_OVERLAY_ITEM_OFFSET`). Never picked |
| `overlays(pos)` / `set-overlays(pos, list)` | Read or replace a host's whole list, at network reach. One host call and one staged op; the host keeps its states, data and generation. Another Experience's block: `not-owned`; not a host: `unsupported-state`; an unknown overlay type or item: `unknown-block`; a slot past 11 or twice, a state the type lacks, or an item too large or off the face: `unsupported-state`; more than 12: `too-large` |
| Block changes | `set-block` clears a host's overlays; `set-block-state` keeps them |
| `on-interact` | Gains `hit`, the click position in pixels when the client sent one inside the cell, and `target`, the slot of the first overlay in slot order whose turned selection box, grown by `OVERLAY_PICK_MARGIN` (0.032 px), holds `hit` |

The adapter saves a host's overlays in its `CinnabarOverlays` block entity, which reaches clients
with the chunk and on every change (format: [`cell_overlays.rs`](../crates/world/src/block_entity/cell_overlays.rs)).
Cinnabar meshes block-state overlays with terrain, draws item overlays with block entities, and
picks overlays with its interaction ray: the nearest hit is outlined and its face and position are
what `UseItemOn` sends. Overlays show only while their host state is the cell's block. Vanilla
clients get the overlay types as hidden palette entries and an unknown block entity, so they draw
the host alone and pick only the host's own selection box: `hit` is where their click met that
box, and `target` an overlay only when that point lies in its box.

## Blocks and items

Paired declarations cover cubes and items; richer guests return `Registration`.
Blocks may declare states, placement traits, indexed geometry, material bindings,
flipbooks and ordered visual permutations. State names belong to the Experience;
placement-trait axes precede authored axes. The first value is the default.
Later matching permutations override earlier ones. A zero rotation cannot undo an
earlier nonzero rotation. Plain cubes have full collision/selection, are harvestable
by hand and drop themselves. See the WIT and [registration validator](../crates/experience-runtime/src/load.rs).

The actor's inventory has hotbar slots first and offhand last. Fresh stacks may
contain own-item data; foreign/ordinary stacks cannot. Vanilla metadata variants,
stack limits and offhand eligibility come from the adapter's item catalog.
Own-item data persists with the stack and reaches clients; decode it as untrusted.
Private block data does not enter chunk NBT.

Server WIT 0.12's `callback.game-mode` reads the actor's game mode, snapshotted with its
inventory (`player-unavailable` without an actor). The adapter reads it from what the mode
allows rather than its type: `spectator` without collision, `creative` with the creative
inventory, `adventure` without world editing, and `survival` otherwise, so the local server's
`/fly`, which lets a mode fly, reads as the mode it wraps.

Server WIT 0.12 adds the `content-rules` export, which runs once after `register`:

| Rule | Contract |
| --- | --- |
| `block-rule.drops` | `itself` (the default) drops the block's item; `nothing` drops nothing, leaving drops to `on-break`. Explosions then drop nothing |
| `block-rule.item` | `listed` (the default) in the creative group; `unlisted` registered but left out; `none` registers no item: no creative entry, pick-block does nothing, no stack of it can be made (`unknown-block`), and it cannot drop itself |
| `block-rule.redstone` | `false` (the default) leaves the block out of redstone; `true` makes it a redstone block that receives and emits power, below |
| `unlisted-items` | The Experience's items that the creative inventory leaves out |
| `callback.harvested` | True only in an `on-break` whose player's game mode drops what it breaks (not creative); guests drop what `nothing` leaves to them only then |

A rule naming an unknown block, two rules for one block, or an unknown or repeated unlisted item
fails loading.

An Experience block takes every click a player gives it without sneaking, as before, unless its
`on-interact` passes the click on with `callback.pass-interaction`, as a vanilla block with
nothing to do with the click does. Once that result commits, the player's held item is used on
the block as if it had no action: a block item is placed against it, and a bucket or flint and
steel is used. Nothing passes when the result is rejected or fails, or when the player holds
another stack by the commit; `pass-interaction` in any other callback is `denied`. A sneaking
player holding an item still bypasses `on-interact` altogether.

`callback.container` reads, and `callback.set-container-slot` stages, the slots of a vanilla
container block in reach: a chest, barrel, shulker box, hopper, furnace, blast furnace, smoker or
brewing stand that is the anchor or a member of the anchor's network, or next to one of them. A
double chest is one 54-slot container at either half. The snapshot lists up to `MAX_CONTAINERS`
positions, the anchor's first and then its network's in breadth-first order, with at most
`MAX_CONTAINER_DATA_BYTES` of item data, in a quarter of the callback frame that the network
leaves them; past those bounds the rest of the reach is `unavailable`, as are unloaded
positions. A written slot takes any stack the guest may make, as `set-slot` does, whatever the
container's own rules for that slot, and a result that writes a container commits only if every
container it writes still holds what its snapshot did. Experience blocks hold no containers.

A block whose rule sets `block-rule.redstone` takes part in redstone; every other Experience
block opts out, so dust neither connects to it nor powers it. `callback.redstone-power` reads the
power, 0 to 15, that such a block of the Experience receives, as its snapshot cell holds it, and
`callback.set-redstone-power` stages the power it emits from every face, as a block of redstone
does: the dust and mechanisms beside it are powered, but not through the blocks beside it. The
adapter keeps the emitted power with the block's store entry, outside its token, so it lasts
across restarts, changes no revision, and goes with the block. When the power a redstone block
receives changes, it gets `on-neighbor-changed` with its own position as `neighbor`, admitted
like every neighbor change at most once per block per tick, so a guest re-reads
`redstone-power` on any neighbor change. Either call on a block that is not a redstone block is
`unsupported-state`, and on another's block `not-owned`.

## Entities and seats

Server WIT 0.11 adds registered entities in the mod's namespace. Definitions select
an existing built-in appearance or indexed geometry, PNG and render-controller assets.
The host validates resources before freezing its world registry and resource packs;
helper restarts must return the same declarations.
Collision boxes must be centered squares at the feet, with width and height that
fit positive packet floats. Offset or rectangular boxes are refused at registration.
Cinnabar resolves a built-in appearance from the actor's admitted constructor when
its custom identity has no authored artwork, including server-pack overrides of that
built-in appearance. Authored custom artwork takes priority.

| Operation | Contract |
| --- | --- |
| `entities` | This mod's live actors in the current instance, including staged writes |
| `spawn_entity` | Reserves a generation-bearing identity; unavailable during final leave |
| `update_entity` | Stages position, rotation, velocity, label, scale, variant and declared properties; teleport selects immediate movement |
| `despawn_entity` | Removes an owned actor and its occupied seats |
| `mount_entity`, `dismount_entity` | Current live player only; occupied/invalid seats and boarding while riding another vehicle are refused |
| `on_entity` | Interaction/attack hand and held item, rider changes, input and removal; player/world epoch fences still apply |

Velocity supplies interpolation in blocks per tick; guest code owns movement rather
than a second physics simulation. Teleports retain the requested velocity. Spawn
identities retain their ordinal across coalesced block writes. Actors keep their
current chunk resident and retain the prior owning chunk until membership migrates
on the next framework tick. Deferred world tasks retry missing destinations even before
that entity can tick; despawn cancels pending retries. Actors and seats are process-local: unload and shutdown
release them and their chunk leases, and terrain saves exclude them. Player travel
leaves actors in their original world and dismounts departing riders.
Removal callbacks retain live entity snapshots while their world opening exists.
After unload, they retain only the historical opening and private stores.
Their private writes still check scope and revisions; entity spawns and world
operations are unavailable in that historical callback.

Seat offsets are relative to the actor. At most one seat steers; input supplies
movement intent, look angles, jump and sneak rather than client vehicle coordinates.
Sneaking dismounts. Late viewers receive links after both actors are introduced.
Removal callbacks retain the removed opening after unload without live actor access.
Operations share `MAX_STAGED_OPS` and `MAX_HOST_CALLS`; registration and snapshots use
`MAX_ENTITY_TYPES`, `MAX_ENTITY_SEATS`, `MAX_ENTITY_PROPERTIES` and `MAX_ENTITIES`.
These are mod execution limits. Mixed-client rendering and live riding acceptance
remain incomplete. The probe's x=34 and x=35 counters create and enter its entity
instance; interact with the ship to board and sneak to dismount.

## Budgets and limits

Use the constants below rather than treating a denied operation as success.
[Runtime limits](../crates/experience-runtime/src/limits.rs) are authoritative;
the adapter's mirrored commit limits are checked against generated fixtures.

| Resource | Source constants |
| --- | --- |
| Fuel and deadlines | `CALLBACK_FUEL`, `REGISTER_FUEL`, `CALLBACK_DEADLINE`, `REGISTER_DEADLINE` |
| Guest memory, stack, tables, instances | `MAX_MEMORY_BYTES`, `MAX_WASM_STACK_BYTES`, `MAX_TABLE_ELEMENTS`, `MAX_CORE_INSTANCES` |
| Artifact and declarations | `MAX_COMPONENT_BYTES`, `MAX_MANIFEST_BYTES`, `MAX_INDEX_BYTES`, `MAX_ARTIFACT_ENTRIES`, `MAX_BLOCKS`, `MAX_ITEMS`, `MAX_STATE_BITS`, `MAX_PERMUTATIONS`, `MAX_OVERLAY_TYPES` |
| Cell overlays | `MAX_CELL_OVERLAYS`, `MAX_OVERLAY_ITEM_SIZE`, `MAX_OVERLAY_ITEM_OFFSET`, `OVERLAY_PICK_MARGIN` ([`overlay_face.rs`](../crates/block-transform/src/overlay_face.rs), shared with the client) |
| Callback work | `MAX_HOST_CALLS`, `MAX_STAGED_OPS`, `MAX_STAGED_DATA_BYTES`, `MAX_SCHEDULED_UPDATES`, `MAX_SCHEDULE_TICKS` |
| Private/item data | `MAX_BLOCK_DATA_BYTES` per block or scoped record, `MAX_ITEM_DATA_BYTES` |
| Messages and tells | `MAX_CLIENT_SENDS`, `MAX_CLIENT_SEND_BYTES`, `MAX_VALUE_DEPTH`, `MAX_TELLS`, `MAX_TELL_BYTES` |
| Networks and recipes | `MAX_NETWORK_BLOCKS`, `MAX_NETWORK_DATA_BYTES`, `MAX_RECIPES`, `MAX_RECIPE_INPUT_ITEMS` |
| Containers | `MAX_CONTAINERS`, `MAX_CONTAINER_SLOTS`, `MAX_CONTAINER_DATA_BYTES` |
| IPC | `MAX_FRAME_BYTES` |
| Adapter lifecycle/storage | `loadDeadline`, `resultDeadline`, `eventQueueCap`, `dataQuota`, `flushInterval` in [adapter limits](../tools/localserver/experience/limits.go) |

`experience-runtime serve --report-fuel` logs callback fuel use. Client grants and
message/byte rates apply in addition to the server callback budgets; see
[client parts](server-experiences.md#permissions-and-budgets).

## Private data store

`<world>/experience-data/<id>.json` stores block data by stable world-instance ID
and position, with placement generations and data revisions, plus scoped records.
Orphaned block data is not returned. Dirty files flush atomically on the configured interval and shutdown.
A crash can lose one interval of writes; world saves and data saves are independent.

| Private record scope | Authorized identity |
| --- | --- |
| `StoreScope::Player` | Callback's stable player ID, including final leave; writes require session authority |
| `StoreScope::Instance` | Callback's current stable instance |
| `StoreScope::Mod` | Installing mod |

Each scope holds one versioned record per identity. `Persistent` and
`Callback::read_state`/`write_state` use serde JSON. `store_read`/`store_write`
expose bytes for explicit migrations: read the old schema before replacing it.
A different schema read fails; absent and empty differ, and `None` deletes.
Deleted records retain revisions. Records stay private unless explicitly sent.

Block and scoped writes validate and commit together, sharing `dataQuota` by net
byte change and the callback's staged-data/operation budgets. Snapshots contain
only authorized scopes. Leave writes retain scope/revision checks without a live
world. The flush crash window still applies.

Bind worlds with stable `cinnabarext.WorldID` values. Opening generations are
reserved in `_worlds.json` before publication, including across restarts.
Store schema 3 adds scoped records to schema 2's instance-keyed blocks; schema 1's
dimensions migrate to the three existing world instances. Unknown or ambiguous
legacy identities fail startup. Back up before upgrading: older servers require the old backup to downgrade.
`_installed.json` records required Experience and block IDs; removing either fails
startup. For an older ID-only manifest, restore original artifacts and explicitly
add a `blocks` map of each recorded Experience to its original block IDs.

## Reload

Server stdin accepts `pause`, `resume`, `experience reload <id>` and `stop`.
Pause holds dispatch and commits. Reload clears strikes, restarts and quarantine,
but must preserve ID, registered blocks/items in order, entity schemas and world declarations;
code and version may change. A failed reload leaves the Experience quarantined.

Guest traps, budget failures and protocol violations count as strikes; guest
rejections, host cancellation, transport and host encoding failures do not.
Unusable helpers are replaced; host-canceled replacements consume no fault
restart. The adapter's strike/restart windows can quarantine one Experience while its blocks and other Experiences remain.
Shutdown stops admission, delivers final leaves and drains callbacks before
flushing stores, reaping helpers and closing worlds. The local server uses a
ten-second drain deadline and reports timeout as an error; a full queue does not
drop final leaves. Custom hosts must call `Host.Drain` with a bounded context
before closing the server or worlds.

## Host protocol and versions

| Axis | Current contract |
| --- | --- |
| Server WIT | 0.11; frozen 0.1–0.6, 0.8, 0.9 and 0.10 worlds remain in [runtime WIT](../crates/experience-runtime/wit) |
| Adapter IPC | 10, `PROTOCOL_VERSION` in [protocol.rs](../crates/experience-runtime/src/protocol.rs); exact match required |
| Manifest data schema | `DATA_SCHEMA` in [artifact.rs](../crates/experience-sdk/src/artifact.rs) |
| Client WIT/wire | [Client compatibility](server-experiences.md#versions-and-wire); [shared drawing](server-experiences.md#drawing-and-signed-assets) |

Server 0.1 has no client messages; 0.2 adds scalar messages; 0.3 adds nested records
and epochs; 0.4 adds focus; 0.5 adds block states/visuals, items, inventories and
networks; 0.6 adds recipes and scheduled updates; 0.8 adds worlds, player events
and scoped state; 0.9 adds ticks and timers; 0.10 adds cell overlays and the click position of
an interaction; 0.11 adds entities and seats. Unsupported callbacks are rejected.

A host runs one `experience-runtime serve` helper per artifact over private stdio.
Frames are a little-endian u32 JSON length followed by JSON, bounded by
`MAX_FRAME_BYTES`; binary fields use lowercase hex. Clear the helper environment,
forward stderr separately, check `loaded`'s protocol, and register content before
freezing the server registry. Never run guest IPC on the world thread.
Snapshot events, validate and commit returned operations atomically, then deliver
messages on declared client routes. Own instance generations, player-session and
epoch fences, scoped snapshots, storage, stale checks, quotas, strikes and shutdown
deadlines. Current Go `OnClientMessage`/`OnEpoch` hooks receive the originating
`cinnabarext.PlayerSession`; match the admitted session before routing input. Use
the [Go adapter](../tools/localserver/experience) and its `testdata/protocol`
fixtures to check a new host.
