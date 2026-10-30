# Personal client mods

Personal mods run under the user's grants in local and remote worlds. Server
consent does not control these grants. For server-delivered client parts, use
[paired mods](paired-mods.md). Personal mods use [`mod-api`](https://github.com/cinnabar-mc/cinnabar/tree/dev/crates/mod-api),
not the Experience SDK's client world.

## Build and load

The [hello example](https://github.com/cinnabar-mc/cinnabar/tree/dev/examples/mods/hello) uses the `extension` world:

```sh
rustup target add wasm32-unknown-unknown
cargo build -p hello-mod --target wasm32-unknown-unknown --locked
cargo run -p mod-host --bin mod-host --locked -- pack \
  target/wasm32-unknown-unknown/debug/hello_mod.wasm /tmp/cinnabar-hello.wasm
cargo run -p mod-host --bin mod-host --locked -- probe /tmp/cinnabar-hello.wasm
```

Import the component in Settings → Mods, review permissions and Enable it.
Reload rereads the installed path; atomically replace that file to update the same
registration. Invalid replacements retain the previous instance. Successful reloads
reset guest memory and preserve companion settings. A guest trap quarantines that
mod and clears its outputs. Reload a valid changed component to recover.

For explicit development selection, set `CINNABAR_MOD_COMPONENT` to an absolute
component path. `CINNABAR_MOD_PACKAGE` takes precedence, and `CINNABAR_MOD_SET` can
select an ordered JSON list. Registered enabled mods run before environment mods.
The client still needs its normal runtime carriers.

## API and grants

[`extension.wit`](https://github.com/cinnabar-mc/cinnabar/blob/dev/crates/mod-api/wit/extension.wit) defines the low-level
world. The `player-mod` world supports package screens and session catalogs;
implement [`PlayerMod`](https://github.com/cinnabar-mc/cinnabar/blob/dev/crates/mod-api/src/guest.rs) and call
`export_player_mod!`. Init and frame are required; event callbacks default to no-ops.
Input edges expire, while latest layout/session changes wait for a callback lane.
Frame, event and load work have separate budgets.

Optional grants default off in [`ModGrants`](https://github.com/cinnabar-mc/cinnabar/blob/dev/crates/mod-host/src/grants.rs).
A component may request them in top-level `cinnabar:grants` JSON; requests grant
nothing by themselves. Commands authorize only the requested names. Settings can
revoke individual grants or all grants; permission changes reload that mod.

Joining, disconnecting or changing connections creates a fresh guest so observations
cannot carry across connection boundaries. Same-connection dimension transfers retain
guest memory and HUD surfaces, including while world observations are unavailable.
World drawing handles, observations and pending world output are cleared on transfer
and rebound to the destination; mods must rebuild their world drawing. Replacements
reuse an in-memory compiled component only for identical mod bytes and engine policy.
Changed bytes invalidate that entry, and unloading the mod releases it.
Explicit environment-selected components get the environment grant and opt into
other grants with the [development switches](https://github.com/cinnabar-mc/cinnabar/blob/dev/crates/client-extensions/src/modding/grants.rs).

| Capability group | Contract source |
| --- | --- |
| Read-only inventory/effects, poses and physics | `player-state` and `gameplay` in extension WIT; unavailable data is optional, not inferred |
| Camera, item use, movement and attack requests | WIT `gameplay`; frame-local policies expire even when a callback is deferred |
| Native damage shake | `camera.read-damage-shake` with `camera`: the last presented shake at full strength, in `view-tilt` degrees, so a suppressing tilt can redraw it at another intensity |
| Raw physical input and player bindings | WIT `input.read-controls` and `input.read-bindings`; `controls`; mods resolve action holds themselves |
| Controls, settings and personal panels | WIT `input`, `settings`, `panel`; settings use a bounded companion file |
| Touch buttons | WIT `touch`; `controls`; labelled buttons in the built-in touch style, placed by players in the touch layout editor |
| HUD surface, text metrics and vanilla elements | WIT `hud`; see [HUD surfaces](#hud-surfaces); retained output clears on quarantine/reload/unload |
| Nametag style, actor annotations and tints, hurt overlay, tooltip lines, chat actions | WIT `presentation`; `hud`, plus `players`/`entities` for per-actor rows; merged across mods in load order |
| Item, food, potion and effect facts, and every installed food | WIT `facts`; `player-state`, with its own per-callback read budget |
| Time/weather and local menu branding | WIT `environment`; changes presentation, not server state |
| World primitives and post passes | WIT `render`; [render example](https://github.com/cinnabar-mc/cinnabar/tree/dev/examples/mods/render-sample) |
| Retained world drawing | [drawing WIT](https://github.com/cinnabar-mc/cinnabar/blob/dev/crates/mod-api/wit/deps/world-draw/drawing.wit); separate material/depth/post/through-wall grants |
| Entity masks | [Mask reference](entity-masks.md) |
| Nearby terrain | [Terrain reference](terrain.md) |
| Block search and fullbright | `terrain.find-blocks` drawn with world drawing; `environment.set-fullbright` |
| Attack/use filtering | Opt-in `filter-actions` export through `mod_api::filtered_player_mod`; `controls` |
| Remaining callback fuel | WIT `budget`; no grant: it reads only the running callback's own allowance |

Callbacks publish only after success. Session changes, reload, revocation and traps
invalidate scoped handles and outputs. Check typed errors rather than silently
assuming an import succeeded. Shared values and ceilings live in
[`mod-api`](https://github.com/cinnabar-mc/cinnabar/blob/dev/crates/mod-api/src/lib.rs),
[drawing limits](https://github.com/cinnabar-mc/cinnabar/blob/dev/crates/mod-api/src/drawing.rs) and
[host limits](https://github.com/cinnabar-mc/cinnabar/blob/dev/crates/mod-host/src/lib.rs).

`gameplay.set-attack-reach` requires `interaction`; a non-`none` request also
requires a current gameplay frame. It sets the local actor pick and attack distance
up to `MAX_ENTITY_REACH_BLOCKS`. `none` restores the current mode and input profile.
The override preserves attack eligibility and block obstruction checks, leaves
mining and block-use reach alone, and does not change the server's attack validation.
The existing generic `interaction` grant covers this request; individual mod
features do not get separate grants.

## HUD surfaces

`hud.set-surface` publishes the mod's JSON-UI layer over gameplay, below vanilla
screens and never hit-tested. It takes the panel surface schema, `screen`,
`document`, `bindings`, `collections` and `transitions`, without input actions,
glass backdrops or `cinnabar_hud_control`. Empty text removes it.
`hud.update-surface` takes `remove_bindings`, `bindings` (replace all),
`binding_changes` (sparse), `collection_order` and `controls`.

`remove_bindings:["#row_7","#row_8"]` unbinds names before the patch's other
bindings apply, which frees room in the binding budget. Each name must be bound and
listed once, the same patch must not also set it, and the list holds at most the
512-binding limit; otherwise the whole patch is rejected. The document may still
read a removed name: it then draws exactly as an unbound name does in a fresh
`set-surface`, and a control that followed a removed projection stays at its
authored place.

`controls` replaces or removes top-level control definitions of the document by
name, without namespace:

```json
{"controls":{"armor_card":{"type":"panel","size":[60,20],"controls":[]},
 "preview_ping@card":{},"old_preview":null}}
```

A key may carry an `@` base as a document key does. The patched document is
admitted exactly as `set-surface` would admit it whole, with the same errors, but
the host rechecks only the replaced definitions and the root children that read
them, so patching a control every frame stays cheap. A patch to the root's own
properties, a rejected patch, and documents with collections, a root base or an `@`
inside a string value are validated whole. Only the controls whose resolution reads
a replaced definition, embedded ones included, are laid out again; everything else
keeps its cache. Values, documents and limits live in
[`surface`](https://github.com/cinnabar-mc/cinnabar/blob/dev/crates/ui/src/mod_panel/surface.rs): 512 bindings, 192 KiB of
document and 64 KiB per update. Only the `screen` root draws on gameplay; other
named controls in the document stay embeddable. The layer hides with the gameplay
HUD, under F1, screens and panels that hide it; it stays under a panel that sets
`show_gameplay_hud`. Several mods' layers draw in load order.

`panel.set-content` sets a panel's `show_gameplay_hud`, and a `panel.update-content`
patch `{"show_gameplay_hud":true}` changes it, so one panel can show the HUD on
only some of its routes. The flag lasts until another patch changes it or
`set-content` replaces the panel.

Each top-level control of the root is laid out, painted and cached on its own, so
a binding change repaints only the controls that read it. A root that draws
itself, a `stack_panel` root or sizes measured from children or siblings keep the
root as one cached control.

Surfaces in HUDs and panels accept two host-resolved values:

```json
{"#helmet":{"item":{"identifier":"minecraft:diamond_helmet","metadata":0}},
 "#home":{"project":{"world":[10.5,64,-3]}},
 "#target":{"project":{"actor":42,"offset":[0,2.1,0]}}}
```

Bind an item value to an `inventory_item_renderer` node's `#item_renderer_data`;
unknown items draw nothing. A projected value becomes the point's `[x, y]` GUI
offset from the root's top left every frame, using the actor's interpolated feet;
bind it to `#offset`. The host also publishes `#home_visible`, true only while the
point is in front of the camera, inside the root and, for an actor, present. Actor
points need `players` for players and `entities` for other actors. When a control's
own `#offset` and `#visible` are its only projected bindings, the host moves its
cached artwork instead of painting it again.

A panel node `{"type":"custom","renderer":"cinnabar_hud_control","control":"armor"}`
draws the named control `armor` of the same mod's HUD document, with the HUD's
bindings. `"control_binding":"#item_card"` reads the name from a text binding or a
collection field instead. The control is centred in the node. By default its
laid-out size is fitted uniformly to the node, so authored padding frames a preview.
`"scale":s` instead draws the laid-out box at `s` times its HUD size.
`"fit":f`, a fraction in (0, 1], fits the control's painted ink instead, ignoring
`scale`: the ink fills `f` of the node along its limiting axis, at most `max_scale`
times its HUD size, 5 by default, so a short caption fills its preview as a card
does. `scale` and `max_scale` accept 0.25 to 5. Each property reads its binding
override first, `#scale`, `#fit` or `#max_scale`; the surface rejects literals out
of bounds, and a bound value out of bounds draws nothing. The bounds live in
[`hud_control`](https://github.com/cinnabar-mc/cinnabar/blob/dev/crates/ui/src/mod_panel/surface/hud_control.rs). A mod keeps
up to 128 named controls built for embedding; the least recently drawn makes way
for another.

Panels lay out in panel units, which differ from the HUD's GUI units with the GUI
scale and a `reference_size` fitted to a small window. `panel.read-layout`, with
`controls`, reports the open panel root's size in panel units, the HUD GUI units one
panel unit spans and the root's offset from the HUD root, so a pointer `p` from
`input.read-controls` lies at `(p - offset) / scale` in panel units.

`budget.remaining-fuel` returns the fuel left in the running callback, roughly the
instructions it may still execute. It needs no grant and exposes no game state; use
it to spread large work, such as encoding a document, over later callbacks.

`hud.read-viewport` reports the HUD root in GUI units and the GUI scale.
`hud.measure-text` returns a line's width at text scale 1 in the `native` or
`sans` face, as a label lays it out. `hud.set-hidden-elements` hides the crosshair,
status effects, sidebar or hunger row; `hud.read-elements` reports whether vanilla
draws each this frame, ignoring hiding, and its bounds in GUI units when laid out.

## Action filtering

A player mod opts into `filter-actions(action-frame) -> action-verdict` with
`mod_api::filtered_player_mod` and the `controls` grant; ordinary `PlayerMod`
exports claim nothing. Only the earliest active, granted exporter runs, after
ordinary frames and before gameplay, with its own fuel and no host imports. It sees
resolved Attack/Use transitions, physical and logical holds, a continuity epoch and
monotonic microseconds, and returns one accept/suppress decision per transition.
Suppressing a release retains the hold; suppressing a fresh press blocks that
gesture until physical release. Traps, invalid verdicts, owner changes, absorbing
UI, suspension and session changes clear continuity and change the epoch. The host
owns projection and lifecycle only; the module owns every filtering rule.
Pulses from `gameplay.pulse-attack` and `item-use.pulse-use` reach the filter as
repeats on a physically held control.

## Touch buttons

With the `controls` grant, `touch.set-buttons` retains up to four labelled buttons,
drawn in the built-in touch style while touch controls are shown. Each has a default
anchor, inward offset and size; players move it in the touch layout editor, and the
placement is saved under the button id, so prefix ids with the mod's name. Across mods
the earliest use of an id wins and at most sixteen buttons are shown. A tap completes
when the finger lifts inside the button without interrupting other fingers, and
`touch.read-presses` returns it during the owner's next `frame`. A mod can then open
its panel with `panel.set-open(true)` or take any other action it is granted.

## Packages and screens

A package directory contains `mod.toml`, `mod.wasm`, `ui/` templates and `textures/`.
Use [`screen-probe`](https://github.com/cinnabar-mc/cinnabar/tree/dev/examples/mods/screen-probe) as the complete example.
The manifest indexes declared files with SHA-256 and names permissions, templates,
actions and keys. IDs and templates must belong to its namespace. The build script
uses `experience_sdk::declarations::generate_mod`, then the guest includes generated
constants with `experience_sdk::include_declarations!()`.

The manifest request and loader grant must both allow each package capability.
Inventory requests also need the explicit inventory development switch. Reload
narrows against the original authorization, so editing a manifest cannot add grants.
The settings companion is `<package>.settings.json`, outside the indexed package.

An overlay draws beside the active container, clipped away from its panels and
excluded areas. A view hides the container's presentation without closing its
server container. Escape returns to the container; closing the container closes
both. `screen_changed` reports layout in GUI units, and `view_closed` reports a
host-closed view. Declared actions, keys, text, wheel events and data revisions
arrive through `PlayerMod` callbacks. UI values and collections use the same
[tagged bindings](https://github.com/cinnabar-mc/cinnabar/blob/dev/docs/server-experiences.md#modal-screens) as client parts.

## Single-file resources

Only binary components support embedded resources. Put each in one top-level
custom section, not inside a nested core module. Resource budgets are separate
from executable bytes; directory packages retain their own lifecycle.

| Section | Payload/grant |
| --- | --- |
| `cinnabar:extension/panel-bundle@1` | Panel JSON; `controls` |
| `cinnabar:extension/font@1` | Outline font; `controls` |
| `cinnabar:extension/menu-logo@1` | PNG; `environment` |
| `cinnabar:extension/menu-title@1` | Single-line UTF-8 heading; `environment` |
| `cinnabar:extension/notices@1` | UTF-8 copyright/license text |
| `cinnabar:extension/icon@1` | PNG Mods-tab icon |

[SDK constants](https://github.com/cinnabar-mc/cinnabar/blob/dev/crates/mod-api/src/lib.rs) define byte/image limits.
Duplicate, nested, malformed or oversized presentation resources are rejected;
invalid icon/metadata affects only the card. `CINNABAR_MOD_FONT` and
`CINNABAR_MOD_UI` sidecars override embedded font/panel data. Mod cards read standard
component name, description, version, authors and licenses metadata.

## Optional GitHub update checks

A component can append top-level UTF-8 `cinnabar:extension/update-repository@1`
containing its public GitHub repository URL for release notifications. For automatic
installation, use `cinnabar:extension/update-url@1` containing an exact latest-release
WASM asset URL of the form
`https://github.com/owner/repo/releases/latest/download/mod.wasm`.
A download URL takes precedence over a repository-only declaration.

Updates require standard SemVer version metadata and the same declaration in the
replacement. Drafts, prereleases and older/equal versions are skipped. Downloads
require GitHub's SHA-256 digest, compatible components and matching versions.
Only unchanged files in the managed mods directory are replaced automatically;
external files can receive notifications. Companion settings and grants survive.
The Mods tab's Update action works when automatic installation is disabled.
Settings → Updates controls mod checks and automatic installation separately.
Mods without either declaration make no update requests.

## Trusted packet mods

`CINNABAR_PACKET_MOD` selects a packet mod on its own; the `packets` grant exposes
session packets and pack insertion through the private core relay. A packet mod
cannot share a registration with gameplay/package mod authority. Grant changes
revoke current authority; reload selects the next session's source. Optional
`CINNABAR_PACKET_MOD_WASI=1` adds clocks, randomness and captured output, never
files or network. See [packet WIT](https://github.com/cinnabar-mc/cinnabar/blob/dev/crates/mod-api/wit/extension.wit) for callbacks.

Native-frame guests must be rebuilt with the current SDK, including guests using
only the legacy `frame` callback. The version-one JSON payload now includes look,
movement and elapsed-time fields; older strict decoders reject these added fields
even though the version number is unchanged.
