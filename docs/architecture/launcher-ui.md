# Launcher models and client UI

Restructuring step 3 extracts the launcher models first, then the retained UI. The
app remains the composition root and keeps its system order, service workers and
network transports. This migration does not introduce a frame of command latency.

## Ownership

| Crate | Owns | App adapter |
| --- | --- | --- |
| `launcher` | Menu actions and views, screen identities, account state, settings models, local-world forms/progress and installation paths | `MenuRuntime`, account/core processes, filesystem operations and world service drivers |
| `client-ui` | Retained HUD/chat/forms/screens, JSON-UI bindings, UI gesture state, item/player previews and presentation | Existing Bevy input, transport, world-query and publication systems |
| `ui-engine` | The portable JSON-UI engine: screen registration, catalog policy, layout caches, painting, texture lookup and the on-demand atlas | `client-ui` supplies the installed vanilla pack and remote downloads through its source interfaces |
| `oreui` | OreUI screens drawn in our own code: design canvas, widgets, motion, transitions, presentation state, artwork loading and shipped fonts | `client-ui` projects menu, loading and bed state into its typed inputs and applies its hits, focus and geometry |
| `ui-screens` | Vanilla screen families over the engine: containers with their recipe and furnace books and hit vocabulary, the HUD, chat, signs, NPC dialogue, credits, loading, toasts, the roster, the emote wheel, and the menu shell's screen and settings bindings | `client-ui` adapters project runtime authority into typed inputs, compose menus, OreUI and the server-form fallback, and supply preview and mod renderers |
| `mod-ui` | Presentation of mod- and server-experience-owned surfaces: consent chrome, the client part's modal, player-mod screens, the personal panel and HUD editor, personal HUD cards and label, package templates and the cosmetic cursor renderer | `client-ui` resolves item icons and container layouts, chooses scene order and passes paint frames |
| `player-state` | The shared composition of inventory authority and local-player facts | `PlayerRuntime` wraps it as the existing Bevy resource |

`UiRuntime` borrows `PlayerState` for projections and commands. It cannot contain
inventory authority or local-player facts. The pre-input presentation snapshot is
still a temporary ledger copy, restored through the existing guard after rendering;
it is not another authoritative inventory owner.

`UiRuntime::session` owns the server's retained UI state in `SessionUi`.
`begin_session` replaces that value when the session id changes; repeating the
current id does nothing. Chat rows, forms, inventory input, HUD state, cached
storage observations and outbound queues retire together. A new server opening
the same coordinates cannot inherit the previous server's storage variant or
request address before the next world capture. Owners with lifetime counters or
user preferences build fresh session values carrying only that retained metadata.
Local language and item catalogs, content-keyed caches, chat history, physical key
state, and the host's loading stage and screen settings remain outside the session.
Cache keys still follow their original source identities and generations, and
changing sessions advances item-name invalidation and dropped-chat accounting.

`ui::IconRef` is now the common icon value used by launcher views and presentation.
The view-radius cap is defined once in `render-api` and reexported by the
`chunk-pipeline` facade. Shared equipment geometry is in `render::equipment`; both
the app's world presentation and the UI preview use those same formulas. The
crossbow icon frame helper is in `inventory` and remains shared with item-use
prediction.

## Temporary seams and ordering

The app's existing modules reexport the extracted APIs, limiting churn for the
later gameplay, session and presentation extractions. These adapters deliberately
remain synchronous:

- `MenuScene` supplies visibility, current view, screen and world/panorama policy
  at the old observation points. Launcher actions still reach the existing app
  navigation and service drivers.
- Inventory ingress still drains in the authority phase. Damage closes eligible
  screens in that phase through the same scene policy.
- `prepare_ui_runtime` observes world readiness, local physics, crafter block state,
  item-use animation frames and nametag picking before outbound actions. The new
  crate accepts those observations, including `ItemIconFrames`, and captures the
  same inventory snapshot.
- `publish_ui_runtime` consumes that capture after input enqueue. Rendering uses
  `render_prepared_ui`, then the app performs the same hand binding, scene publish
  and fatal-error handling.
- Form/chat/inventory network flushes retain their queue order, backpressure and
  session checks. UI sound requests keep their bounded queue and are drained at
  the original audio phase.

`client-ui` reads admitted world facts through three borrowed `client-world::ui_read`
contracts: `InventoryUi` for container admission, stations, mount HUD and item publication;
`LocalPlayerUi` for the local profile, skin and evaluated equipment/body pose; and
`NametagUi` for actor labels and ride relationships. Queries remain lazy and
synchronous at the existing frame observation points. Shared profiles, skin/pack
allocations, NBT and registry identity tokens retain their original owners.
Container block queries accept the existing global `world::BlockPos`, keeping
block positions distinct from chunk and section keys.

Shared block-crack observations belong to `client-world`; the app captures them
after the stream advances progression and passes the snapshot to UI. Scheduling,
worker queues and world mutation stay outside UI. `chunk-pipeline` is only a UI
dev-dependency for existing integration fixtures. Draw and preview types come from
`render-api` and `render-model`. UI compilation fixtures use `pack-compiler` for UI
carriers and `pack-entity-compiler` for actor artwork, while `asset-compiler` remains
the offline CLI.
The gate keeps reusable compilation out of UI production and out of launcher
models, including their test dependencies. The UI's extension chrome retains
`mod-host` and `server-experience`. These are explicit temporary dependencies in
the policy, to be narrowed by later presentation/session migrations. Launcher
models and player state have no Bevy or renderer dependency.

## Enforcement

`tools/architecture/policy.toml` records the new dependency allowlists and rejects
transitive Bevy, renderer and client-UI backedges from launcher and player state.
It rejects app, runtime, session and gameplay dependencies from client-ui.
A production dependency rule also rejects streaming, meshing and far-terrain routes
from client-ui, including transitive, renamed, workspace-inherited and target-specific
build edges. The same packages remain available to test-only fixtures. Source rules reject
app module access from the new crates, UI-owned inventory/player authority, and
movement access to UI (including the new crate name).

Shared app test fixtures require client-ui's non-default `test-support` feature.
The gate rejects that feature in production dependencies or feature forwarding,
including renamed and workspace-inherited dependencies. The same rule protects
`publication-test-support` on `chunk-pipeline` and `render`, including the
`client-world` dependency alias. Module alias checks are
scoped to each owning Cargo crate so an identically named alias in another crate
cannot hide a forbidden edge. Protected production modules cannot use `#[path]`
remapping, including conditional attributes; test-only remaps remain allowed.
The source gate checks syntax and aliases, without expanding arbitrary macros.
Marker checks follow the moved producer crate and
still reject undeclared or duplicate identifiers. Boundary regression tests cover
these cases.

## Behavior evidence and references

This is a relocation of existing behavior, not a new parity claim. The source
migration audit retained all 624 original UI test names and all 216 launcher,
local-world, store and installation test names. Store-only coverage is removed with the
marketplace. Later upstream audit deletions,
consolidations and fixture guards are preserved at their moved locations. Tests
needing app resources or services remain app integration tests; pure projections
and reducers move with their owner. The launcher constructor comparison checks
that shared view defaults
still match the actual runtime for both visible and hidden menus.

The screen-policy witnesses retain the vanilla pack's settings, including the
HUD's `should_steal_mouse: true` and `absorbs_input: false` at
`.local/assets/bedrock-samples/v1.26.50.4/full/resource_pack/ui/hud_screen.json:3576`.
Mouse ownership is delegated to the current screen. Current-version screen mouse
policy remains unverified; this migration makes no new parity claim.

See [UI-edit executable build measurements](../evidence/launcher-ui-build-timings.md).
No game server or client session is used for validation.
