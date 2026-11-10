# Server client parts

A client part is optional code offered by a server. Build a paired mod with
[Cosmic](modding/src/appendix/paired-mods.md); host server callbacks with the
[Experience runtime](experience-runtime.md). Personal client mods have
[separate user grants](modding/src/appendix/personal-mods.md).

Execution currently requires `CINNABAR_DEV_SERVER_EXPERIENCES=1`. Production
restricted launch is unavailable: consent alone does not enable production
execution. Developer helpers are separate processes, without an OS sandbox; the
[restricted helper](#restricted-helper-linux) is not yet used by the client.

## Delivery and consent

The server advertises `cinnabar/extension-offer.json` in an optional resource pack.
The marker carries an Ed25519 server key and signed offer. The offer binds the
join destination, deployment revision, expiry, publisher keys, immutable bundle
digests, HTTPS origins, permission/budget scope and ordinary-client fallback.
Servers must retain ordinary gameplay when client parts are declined or fail.
The client currently cannot independently check the pack's optional provenance.

Consent choices are Allow once, Always, Never and Not now. Always pins the exact
destination, server key, scope and publishers; broader authority prompts again.
Never suppresses that destination, including changed keys. The prompt holds join
readiness, and approval requires its disclosure to have been drawn and scrolled
to the end. F9 revokes the offered/running extension. Trusted status and consent
cannot be replaced by resource packs.

An accepted resource pack grants no code authority. The client sends Hello only
after consent, verifies the signed live Accept, then checks/downloads bundles and
initializes helpers before Ready. Servers must wait for Ready before sending
application records. Timeout, denial, expiry, revocation or fatal helper failure
ends extension execution while ordinary gameplay continues.

Trust settings are in `server-experiences.json` under user data. Invalid settings
fail closed. Immutable bundle bytes may stay cached after leaving; grants, guest
state and pending output do not. Content updates take effect on rejoin.

## Permissions and budgets

Permissions must be in the signed manifest, offer, client capabilities and actual
grant. Requesting a permission does not supply an unimplemented adapter.

| Permission | Client contract |
| --- | --- |
| `ui` | Owned text labels; empty text removes one |
| `modal_ui` | Signed JSON-UI modal templates, values and collections |
| `input` | Declared focused-modal actions, text and modifier changes |
| `messaging` | Signed typed channels; client sends only `to_server` |
| `scene` | Declarative objects sharing the bundle's drawing snapshot and budgets |
| `media` | Playback of indexed signed descriptors |
| `items` | Read-only session item catalog |
| `world_draw` | Shared retained drawing, immediate primitives and local visual time/weather overrides |
| `entity_observe`, `entity_highlight` | Current native actor references and masks |
| `render_material`, `render_post`, `render_through_walls` | Bounded materials, post passes and through-wall drawing |

Client parts share the personal-mod drawing, render, environment and signed-asset
imports. Terrain, packet interception and local settings remain personal-mod APIs.
Client-part grants still come from the signed offer and consent.

| Limit | Authority |
| --- | --- |
| Callback/session fuel and cadence | `CALLBACK_FUEL`, `SESSION_FUEL`, `CALLBACK_INTERVAL_MS` in [runtime.rs](../crates/server-experience/src/runtime.rs) |
| Guest/session memory, output, bundles | `MAX_GUEST_MEMORY`, `MAX_SESSION_MEMORY`, `MAX_HOST_OUTPUT`, `MAX_BUNDLES` in [policy.rs](../crates/server-experience/src/policy.rs) |
| Declarations and signed manifest | `MAX_ACTIONS`, `MAX_CHANNELS`, `MAX_FILES`, `MAX_MANIFEST_BYTES` in policy |
| Typed messages, rates, queues, nesting | `MAX_MESSAGE_BYTES`, `MAX_MESSAGES_PER_SECOND`, `MAX_BYTES_PER_SECOND`, `MAX_BURST_BYTES`, `MAX_QUEUE_*`, `MAX_FIELD_DEPTH` in policy |
| Screens and text | `MAX_TEMPLATE*`, `MAX_TEXTURE*`, `MAX_COLLECTION*`, `MAX_ROW_FIELDS`, `MAX_UI_*`, `MAX_EDIT_TEXT_BYTES` in policy |
| Media | [media.rs](../crates/server-experience/src/media.rs) |

Callbacks stage output; successful callbacks publish whole transactions owned by
session, bundle, generation and world epoch. Traps discard that callback's output.
Per-session budgets prevent multiplying allowances by adding bundles. Initialization
also consumes the aggregate budget; Ready waits for all initializers.

A failed callback restarts its guest and counts a strike. Repeated failures stop
that part; other parts continue. Unusable helper output stops its producer; malformed
session framing ends the extension.
No WASI, arbitrary network, filesystem, account tokens or raw packets are imported.

## Callbacks and registry confirmation

Implement [`ClientPart`](../crates/experience-sdk/src/client.rs).

| Callback | Meaning |
| --- | --- |
| `init` | Initialize once after generated registry confirmation |
| `#[on_message]` | Decoded to-client message; generated dispatch validates the declared channel |
| `frame` | Optional presentation callback; coalesced while busy |
| `epoch` | World epoch changed; state remains live |
| `action`, `secondary_action` | Declared primary/secondary modal action, with collection row |
| `modal_resized` | First drawn size or GUI-size change |
| `text_changed` | Latest text per declared edit control |
| `scroll_changed` | Latest range per declared scroll view |
| `shift_changed` | Shift state while the modal owns focus |

Generated projects return `shared::REGISTRY`. Callbacks wait for its exact server
fingerprint, including after recovery. Matching confirmation initializes once;
mismatch stops the part and clears queued callbacks. While waiting, the host
retains latest state plus ordered messages/actions. Queue overflow stops that part.
After confirmation, latest epoch/modifiers/size/text/scroll state precedes queued
events. Old-epoch events are discarded.

## Drawing and signed assets

Use the [shared drawing contract](modding/src/appendix/drawing-helpers.md) for retained meshes,
materials, camera anchors, primitives and actor masks. References bind to the native
connection, dimension and actor incarnation. World changes clear old presentation;
stale references and dependent draws are dropped and counted without stopping the
part. Frames rotate across ready producers within the shared callback allowance.

Legacy objects and drawing publish in one immutable snapshot. Their draw and
triangle limits combine. Each bundle's GPU ledger covers retained and immediate
geometry, per-view instances, masks and media. Resident storage stays charged until
its last GPU use; buffer growth waits for old storage to retire when needed.

Low-level `[client].assets` declares signed drawing files beside `experience.toml`.
Generated declarations expose `Asset` constants; `assets.read_range` streams larger
files across callbacks. A file listed as a template, texture and asset is packaged
once. Source-driven projects retain their generated `shared::assets` declarations.
The [space client](../examples/experiences/space-client) shows a camera-anchored
planet with signed textures and a bounded procedural material.

## Modal screens

Low-level bundles declare `templates`, `actions` and `textures` in the signed
manifest. `ui.open_screen` accepts only an indexed `ui/<name>.json`; `None`,
`close_screen` or Escape closes it. The template namespace must match the bundle
ID after replacing characters outside `[a-z0-9_]` with `_`. A bundle declares at
most `MAX_ACTIONS` (1024) actions; a press costs one lookup in that set.
Templates resolve against the shipped UI carrier, excluding server packs and
trusted consent catalogs. Images resolve from indexed `textures/`, then the
supported local art sources. URLs and paths outside `textures/` draw nothing.

A modal draws over gameplay only when no other screen is open, below trusted
status/toasts. While drawn, it owns pointer and keyboard input; F9 still revokes.
Bound values survive closing/switching screens.

| Binding/control | Contract |
| --- | --- |
| `ui.set_value` | Global `#name`: boolean, integer, finite number, text or bounded numeric array |
| `ui.set_collection` | Replace collection rows; each maps `#name` to a tagged value |
| `inventory_items`, `hotbar_items` | Host-owned read-only stack collections |
| `$pressed_button_name` | Declared action delivered on release |
| `button.menu_secondary_select` | Maps a secondary press to a declared action |
| `text_box_name` | Declared edit action; whole text coalesces per control |
| `ui.set_text` | Updates text within `max_length` without `text_changed` |
| `scroll_view_name` | Declared scroll action; reports offset, viewport and content in GUI units |
| `ui.modal_size` | Drawn root size in GUI units and GUI scale, otherwise none |

For full UI fixture code, see
[`terminal-client`](../examples/experiences/terminal-client).
The source-driven paired builder generates channels and assets; authoring modal
manifest entries uses the low-level bundle path below.

## Restricted helper (Linux)

`Helper::spawn_restricted` starts `mod-host restricted-server-helper` contained; other platforms
refuse it. The parent first creates a cgroup for the helper, under a random name, below the
nearest ancestor of its own cgroup that delegates the memory and pids controllers to its user, as
a systemd user session does. It sets `memory.max` to 1.5 GiB and `memory.swap.max` to 0, so every
page the helper is charged for, shared memory files included, counts and none can spill to swap,
and `pids.max` to 8 tasks, so threads cannot exhaust the user's task budget. Where the cpu
controller is delegated too, `cpu.max` holds the helper to one core; where it is not, the launch
goes ahead and the helper's CPU time is unbounded, limited only by the callback deadline. Without
the memory and pids controllers the launch fails closed with that reason. The client holds a lock
on the cgroup's directory while it is in use and removes the cgroup once the helper is reaped. A
later launch clears cgroups whose client is gone (unlocked, with their limits set): it kills any
helper still in one through `cgroup.kill`, which also catches a helper that somehow outlived its
death signal, and removes the cgroup once empty.

Between `fork` and `exec` the child joins that cgroup; the parent clears the environment, starts
it in `/`, has `exec` close every descriptor but the three stdio pipes (marking each close-on-exec,
so a later failed step still reaches `spawn`; where `close_range` cannot, every descriptor up to
the hard limit), sets `PR_SET_NO_NEW_PRIVS`, makes the child undumpable and killed with its parent
(refusing to run if that parent died first), and sets resource limits: 1 GiB of private writable
memory and of file size (`RLIMIT_DATA`, `RLIMIT_FSIZE`) under the cgroup's limit, 64 descriptors,
and no core files, locked memory, message queues, queued signals or real-time priority. Because `exec` makes an
ordinary program dumpable again, the helper first makes itself undumpable once more.

Before reading the component, the helper audits that it runs a single thread (Landlock covers only
the calling thread and those it starts), holds no other descriptor (listing `/proc/self/fd`, so
one numbered above the new limit cannot escape, and failing without it), no environment, those
limits and its cgroup's memory and task limits. It then denies itself everything else: a Landlock ruleset
that handles every right the kernel knows and grants none (no file or directory can be opened;
TCP bind and connect on ABI 4, signals and abstract sockets outside itself on ABI 6, and any right
a later ABI adds, since each right is probed from the running kernel), and a seccomp filter, on
every thread, failing with `EPERM` program execution, process creation (threads remain), every
socket call, tracing and cross-process memory, signals to any process but the helper itself (on
every kernel: `kill`, `tgkill` and queued signals only to its own process id, no `tkill`, process
descriptors or I/O signal owners, so signal isolation does not depend on Landlock's ABI 6 scope,
which stays as a second layer), other processes' resource limits, priorities and scheduling,
changes to any file's mode, owner, times, extended attributes, inode flags or length (by path,
which Landlock does not cover, or by descriptor), System V IPC and POSIX message queues,
namespaces, mounts and their listings, BPF, performance counters, keys, io_uring, modules,
quotas, the x86-64 local descriptor table and administration calls, and the `prctl` options that
would drop its death signal, make it dumpable, name a tracer or adopt orphans. Every call
numbered above the newest one reviewed (`listns`) fails with `ENOSYS`, so calls a later kernel
adds stay closed until reviewed; a foreign architecture or x32 call kills it. A helper that cannot lock down exits before reading the
component, and a kernel without Landlock refuses. The component compiles after the lockdown, so
the compiler is contained too, under the same limits and the parent's callback deadline.
`crates/mod-host/tests/restricted_faults.rs` injects each forbidden operation into a contained
child, and `tests/helper.rs` runs a real component restricted; both skip, naming what is missing,
on a kernel without Landlock or without delegated memory and pids controllers.

Known gap: the helper can still learn metadata the lockdown does not hide. Path-based `stat`,
`statx`, `access` and `readlink` report whether files exist and their sizes, owners and times
(Landlock does not cover reading metadata), `inotify` watches can observe activity on paths, and
`get_robust_list` reads the robust futex list address of another process of the user, which can
reveal its memory layout. None of these reads file contents or changes anything.

Production stays unavailable: macOS and Windows have no restricted launch, the Linux launch has
not had the independent security review the gate requires, and the client does not call it.
Wasmtime and native decoders remain attack surfaces.

## Bundles and low-level tooling

A `.cxb` is an indexed ZIP of portable Wasm and assets with
`manifest.signed.json`. Every other file has an exact size and SHA-256 in the
publisher-signed manifest. Paths are portable lowercase relative paths; traversal,
links, duplicates, encryption and unsupported ZIP metadata are rejected. Only
stored/deflate entries are accepted. Native Wasmtime artifacts are refused.
Signatures establish the chosen publisher and integrity, not code safety.
The signed manifest is at most `MAX_MANIFEST_BYTES` (4 MiB) and its payload,
which the document hex-encodes, at most half of that: enough for `MAX_ACTIONS`
actions at the 96-byte identifier limit and `MAX_FILES` index entries at the
256-byte path limit, so those limits bind first. The client parses it once when
it admits the bundle.

`cinnabar-cxb build --experience <experience.toml> --component <guest.wasm>
--publisher-seed <seed> --out <id>.cxb` reads identity/version and the `[client]`
table, componentizes the guest, signs and verifies the archive. `keygen <seed>`
creates a private publisher seed. `seed-cache --cxb <id>.cxb --user-data <dir>`
preloads a verified bundle. Cache reads rehash bytes; cache entries carry no grants.

For a low-level local-server deployment, supply `-extension-key <seed>`,
`-extension-audience <host:port>` and `-extension-cxb <dir>` together. The audience
must exactly match the address players join. The local server advertises a cache-only
bundle origin; seed each client's cache. Offers expire, so restart at least daily.
Paired `mod run` prepares keys, installation and cache automatically.

## Versions and wire

| Axis | Contract/source |
| --- | --- |
| Handshake API | `API_VERSION` in [policy.rs](../crates/server-experience/src/policy.rs) |
| Envelope versions | `WIRE_VERSION` through `MAX_WIRE_VERSION` in policy; v2 negotiates fragmentation and ceilings |
| Client WIT | [capabilities.wit](../crates/experience-sdk/wit/client/deps/server-experience/capabilities.wit), currently 1.5; older 1.x components still link |
| Exported world | [server-bundle](../crates/mod-api/wit/client.wit) |
| Canonical fields/domains/limits | [extension.json](../crates/server-experience/schema/extension.json) |

Transport uses Bedrock `ScriptMessage` with `message_id = "cinnabar:extensions/v1"`.
Both packet-header subclient routes and the envelope must agree; only route zero
is supported. No marker means no extension probe. Before consent, extension
messages are dropped before world sequencing.

Signed payloads use exact compact serialization of the versioned Rust structs,
not JCS. Keys, signatures, digests and nonces are lowercase hex. Preserve field
and array ordering; sign the document's domain bytes plus canonical payload.
Use [generated document types](../tools/localserver/cinnabarext/doc.go) and
`testdata` fixtures for another language's signer, rather than reconstructing
encoding from prose. `cinnabar-cxb write-fixtures <dir>` regenerates the Rust fixtures.

Wire v2 keeps client state on world-epoch changes and fragments oversized records
within negotiated limits. Sequences, routes, schemas, epochs, reassembly budgets
and rates are checked. Wire v1 falls back on dimension change. Neither version's
fallback may depend on a final message arriving from a stopped client.

Server-driven transfers, including same-dimension moves, use an `epoch` notice.
The client replies with `world_ack`: `requested_epoch` echoes the notice and
`world_epoch` is a strictly newer client epoch. Deploy matching client/helper
revisions. The helper fences both directions before delivery; input must match the
acknowledged epoch. Client state stays live and `epoch` requests fresh state.

Committed admissions queue per connection and publish in order at activation or
commit. Acknowledgment retires that notice and all earlier admissions; traffic
opens only with an empty queue and no preparation. Aborting preparation preserves
committed admissions. A client part activating during preparation joins the fence;
after commit it receives the destination notice.

Native epoch updates consume ordinary data credits and wait ahead of later sends.
Queued notices and matching acknowledgments bypass data credits. Notices must fit
256 encoded bytes and exceed the last acknowledged notice; equal/older notices
are dropped. The client advances its acknowledged epoch before writing the reply.
Multiple notices can precede one cumulative acknowledgment. Retired/repeated
acknowledgments use ordinary credits and leave the fence unchanged. Failed notice
writes end the optional session without a retry timer; travel remains committed.
The fence does not certify that terrain has rendered.

## Developer media

`media.control` names an indexed descriptor; a scene quad whose texture is that
descriptor path displays it. Media callbacks use `cinnabar.media` records with
path, event (`playing`, `paused`, `stopped`, `ended`) and position in milliseconds.
The decoder profile is progressive WebM with AV1/Opus, 8-bit I420, limited-range
BT.709, bounded to 720p/30 fps and mono/stereo 48 kHz. Signed chunk hashes are
checked before decode. Whole-object reconciliation after streaming is incomplete.

Build `mod-host` with `media` for the decoder helper. Windows execution is currently
unavailable. Local HTTPS media requires `CINNABAR_DEV_MEDIA_CA` naming the server's
CA and the developer execution switch. Localserver's `-extension-media <dir>` and
`-extension-media-addr <loopback:port>` serve files and write the CA under the world.
`cinnabar-cxb media` strips WebM tags and writes a descriptor.

Playback uses the client media clock. Server-clock wire routing, seamless loops,
sparse seeking and UI/entity surfaces are incomplete; seek decodes from the start.
Use ordinary posters, captions and controls when media is unavailable.
