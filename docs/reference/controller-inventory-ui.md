# Controller inventory and container UI specification

Status: design plus implemented navigation/action foundations; app wiring and
Bedrock controller acceptance remain incomplete. Researched 2026-10-25.

## 1. Scope and evidence rules

Goal: operate inventory, crafting and supported container screens entirely with
a controller, with Bedrock-style navigation, item operations and contextual
prompts. This is not keyboard/mouse emulation through OS input injection.

The UI implementation must be platform-independent Rust over normalized gamepad
input. The local macOS Switch-report adapter is only a hardware event source;
do not put inventory behavior in it. Other controller backends are separate work.

Research baseline: Cinnabar `4e57fe394ee74431ef848a06fe56dd0886faaa90`
(the installed 0.1.28 release), plus local macOS adapter/camera changes. Re-audit
against the implementation branch before assigning edits. Target versions come
from `assets/bedrock-target.json`, not the Cinnabar version: currently game
1.26.50, server 1.26.52.3, wire 2193. UI source is separately pinned by
`assets/vanilla-source.json` to Mojang samples v1.26.50.4.

Evidence classifications used below:

- **Pack**: explicitly authored in the pinned official UI JSON. Establishes
  properties/mappings, not undocumented native algorithms or physical buttons.
- **Code**: inspected in the Cinnabar or pinned Controlify checkout.
- **Secondary**: public wiki/documentation; useful context, not target-version proof.
- **Proposal**: desired Cinnabar behavior pending reference capture. Never relabel
  a plausible interaction as confirmed Bedrock behavior.

Controlify is a **Java Edition** integration and LGPL-3.0-or-later. Study its
architecture/behavior; write original Rust. Do not copy its code or assets without
a separate license/attribution review. It is not authority for Bedrock transactions.

## 2. Current implementation audit

| Area | Finding and source |
| --- | --- |
| Inventory open/close | Implemented through persisted `BindingId::Inventory` in `app/src/ui_runtime/interaction.rs`. This is not slot interaction. |
| Main menu controller navigation | Implemented: D-pad focus and activation in `crates/client-shell/src/menu/input.rs`. Do not assume this routes into containers. |
| Other controller-aware UI | Chat, emotes and server forms have separate paths; e.g. `app/src/ui_runtime/forms/interaction.rs`. These are integration precedents, not inventory coverage. |
| Open-container input | `app/src/ui_runtime/interaction/inventory.rs::apply_inventory_pointer` reads `Window::cursor_position`, mouse edges, keyboard presses and wheel input. No gamepad navigation/cursor/action reducer is wired here. |
| Inventory state | `crates/client-ui/src/ui_runtime/screen_state.rs` retains hover, pointer gestures, scroll/search and station state; no container gamepad navigation state is present. |
| Focus infrastructure | `crates/json-ui/src/input/{focus,navigate,mapping}.rs` already understands authored focus metadata, overrides, containers, precedence and input-mode conditions. Reuse it. |
| Visible targets | Engine container frames/hits and `crates/ui-screens/src/container_actions.rs::InventoryCellHit` address player/storage/armor/offhand/crafting/output/recipe/widget targets. |
| Item operations | `crates/client-ui/src/ui_runtime/{inventory_actions,inventory_drag,interaction}.rs` and `crates/inventory/src/inventory_ledger/` already implement substantial pointer/semantic behavior. They are reusable owners, not proof every station is complete. |
| Controller cursor settings | The official pack exposes them; no `gamepad_cursor_sensitivity` implementation was found in the inspected launcher settings definitions. Do not reuse camera sensitivity. |
| Naming trap | Domain types such as `inventory::InventoryController` own screen preferences; their name does not mean hardware-controller support. |

Conclusion: **partially implemented**, not absent and not controller-complete.
Opening inventory currently does not establish that D-pad, sticks, take/place,
quick transfer or crafting are available without a mouse.

The subsequent foundation implementation lives in
`crates/client-ui/src/ui_runtime/controller_inventory/`: a pure navigation reducer
and semantic action dispatch through the existing ledger, with regression tests.
Dispatch requires a captured screen token and matching resolved/current semantic
selections, including layout revision. These modules are registered but are not
yet called by the app's gamepad inventory path. Physical bindings, effective
virtual pointer routing, focus/cursor rendering, scroll reveal, magnet behavior
and live controller-only acceptance remain open. No installed-app behavior is
changed by these foundations alone.

Reuse existing ledger entry points, including `begin_target_gesture`,
`begin_quick_move`, `begin_gather`, `begin_drop`, crafting/recipe-placement and
station-output operations. Preserve existing admission checks; never directly
rewrite stack counts or invent new controller-specific network packets.

## 3. Bedrock evidence and unresolved bindings

Pinned source links use the exact samples commit in References.

### 3.1 Navigation and cursor: established versus unknown

**Pack:** `ui_common.json::container_slot_button_prototype` enables focus,
disables slot focus wrapping by default, enables focus magnets and supplies
collection-bound identifiers/directional overrides. Overrides are data, not a
license to hard-code nine-column navigation across every screen.

**Pack:** `ui_common.json::{gamepad_cursor_image,gamepad_cursor_button}` defines
a cursor-following visual/input control. `settings_sections/controls_section.json`
exposes hide-cursor, cursor-sensitivity, clear-hotbar and A/B / X/Y swap settings.
`furnace_screen.json` includes the gamepad cursor.

**Secondary:** Minecraft Wiki describes joystick-controlled GUI cursor movement
and a separate Controller Cursor Sensitivity option.

**Unknown:** exact stick roles on each target platform/profile; native movement
curve, magnet radius/engagement timing, D-pad repeat delays, tie-breaking,
cross-panel routing and focus restoration. JSON metadata does not prove these.

### 3.2 Authored slot actions

These are **logical IDs**, not physical controller-button assignments.
`inventory_screen.json::inventory_container_slot_button` maps:

| Source ID | Target ID / operation family | Evidence boundary |
| --- | --- | --- |
| `button.menu_select`, `button.menu_ok` | `button.container_take_all_place_all` | Press mapping is authored. |
| `button.menu_secondary_select`, `button.controller_select` | `button.container_take_half_place_one` | Exact odd-count rounding/context behavior still needs witnesses. |
| `button.menu_auto_place`, `button.controller_secondary_select` | `button.container_auto_place` | Do not infer destination ordering from the name alone. |
| `button.menu_inventory_drop` / `_drop_all` | `button.drop_one` / `button.drop_all` | Safety and physical bindings must be verified. |
| Double press of select/OK | `button.coalesce_stack` | Only controls whose resolved mapping includes it; no blanket result-slot gather. |
| `button.menu_cancel` | `button.try_menu_exit` | Delegate carried-stack cleanup to existing close logic. |

The no-coalesce variant omits gather. Creative hotbar slots override auto-place
to auto-destroy. Recipe cells override primary/secondary/tertiary to
`button.recipe_select`, `button.recipe_secondary`, `button.recipe_tertiary`;
crafting output overrides them to `button.crafting_output_*`.
`furnace_screen.json` has its own slot prototypes and recipe/controller mappings.
There is also a conditional PS4 `button.controller_back` mapping: this alone
invalidates assuming one universal face-button table.

**Physical defaults are an evidence gate.** Xbox-style A=primary, X=half/one,
Y=quick-transfer and B=back are capture hypotheses, not approved defaults.
Record actual target-platform prompts and behavior, including Nintendo face-label
ordering and configured swaps. Define physical positions separately from glyphs.
Never infer a button solely from an ID containing "select" or "back".

## 4. What to borrow conceptually from Controlify

Inspected commit: `68dba946a2732c0ab2112fddbc3abe05a491a565`.

| Reference | Useful design; not a Bedrock parity claim |
| --- | --- |
| `virtualmouse/VirtualMouseHandler.java` | Vector-length easing (avoids distorting diagonals), separate cursor sensitivity, refreshed snap targets, nearest-target snap on stick release, directional snapping, screen bounds and scroll actions. |
| Same file, directional snapping | Direction-filtered candidates, orthogonal-deviation penalty and wrap fallback. Do **not** copy that wrap fallback over Bedrock's authored no-wrap rules. |
| `screenop/ScreenProcessor.java` | Screen ownership, component focus, tab navigation, scroll reveal, mode transitions and specialized screen processors. |
| `screenop/compat/vanilla/AbstractContainerScreenProcessor.java` | Explicit pickup, half/one and quick-move actions; guides derived from hovered slot/carried item; separate compatibility handling away from item slots. Java `ContainerInput` values are not Bedrock requests. |
| `screenop/compat/vanilla/RecipeBookScreenProcessor.java` | Recipe pages/categories handled explicitly rather than pretending all controls are item slots. |
| `mixins/feature/virtualmouse/snapping/AbstractContainerScreenMixin.java` | Targets derived from live slots rather than a static screen-coordinate list. Its pixel offsets/radii are Java-specific. |
| `utils/HoldRepeatHelper.java` | Press-immediate, delayed-repeat state machine. Its tick counts are not measured Bedrock timings. |
| `docs/content/reference/builtin-bindings.md` | GUI navigation/activation, inventory actions and virtual-mouse actions are distinct binding contexts. |

## 5. Proposed interaction contract

### 5.1 Ownership and contexts

One UI input owner at a time, with an active controller identity. Opening a
container transfers authority from gameplay; navigation must never walk the
player, turn the camera, drop a world item or cycle the world hotbar accidentally.
Modal overlays/text entry take precedence over the underlying container.

Route normalized button **edges**, held state and axes into semantic UI actions.
Do not poll only `just_pressed` for every action, and do not replay a button held
while opening the screen as a fresh item action. On close, keep consumed edges
consumed through the gameplay phase and require release before rearming.

Switch to mouse ownership only on meaningful real movement/clicks, not a stale
OS cursor or noise. Stick drift must not steal mouse focus. Mode changes,
disconnect, focus loss, window replacement and server-driven close cancel local
repeat/drag/armed UI gestures without generating a drop or craft operation.
Do not discard authoritative carried stacks on these transitions.

### 5.2 Two coordinated navigation modes

- **Directional focus:** D-pad steps among admitted controls/slots. Respect
  authored focus overrides, focus-container rules, stop/no-wrap and precedence
  before geometric fallback. Never select hidden, clipped-out or disabled hits.
- **Analog cursor:** move a client-owned virtual cursor in GUI coordinates with
  separate cursor deadzone/sensitivity and frame-time-based velocity. Proposal:
  precision near center, faster traversal at large deflection. Exact stick roles
  and response curve remain capture-gated.
- Both resolve to the same current semantic target. A focus step updates the
  cursor/highlight coherently; analog movement does not leave a stale focused
  slot that receives the next action instead of the hovered slot.
- **Magnet proposal:** snap only to enabled authored magnet targets, with an
  escape rule/hysteresis, no oscillation and no jumps to distant panels. Stick-
  release snapping is a Controlify-inspired candidate, not established Bedrock
  timing. Measure before assigning constants.
- Repeat uses caller-supplied monotonic time, deterministic direction arbitration
  and release/reset behavior. Opposite directions cancel; establish diagonal
  behavior from captures. No sleeps or frame-count-dependent repeat in logic.

Use current resolved geometry and authored metadata, not hand-maintained slot
coordinates. Preserve focus by stable window-generation/semantic target identity,
not just a visible row index. If a recipe list filters, do not activate a different
recipe that inherited the old index. Revalidate action targets against current
screen/ledger authority, even when focus survives a layout revision.

Scrolling must reveal newly focused controls, preserve the focused semantic item
where possible, and not continuously undo manual scrolling. In a virtualized
list, materialize/reveal a neighboring logical item rather than skipping every
offscreen item or cycling only the visible page. Resize, DPI, GUI scale, text
reflow and server pack layout changes invalidate stale geometry, not item identity.

### 5.3 Semantic actions and capability checks

Proposed action vocabulary (names illustrative, agree at the contract task):
`Navigate`, `MoveCursor`, `Activate`, `HalfOrPlaceOne`, `AutoPlace`,
`Gather`, `DropOne`, `DropAll`, `NextTab`, `PreviousTab`, `Scroll`, `Back`.
Crafting/station-specific meanings come from the resolved screen mapping.

Do not make gamepad actions anonymous left/right clicks globally. Existing
pointer reducers may be reused internally where semantics genuinely match,
but held/repeated input, coalescing and station outputs need explicit policies.

| Context | Required behavior / invariants |
| --- | --- |
| Empty carried stack + occupied ordinary slot | Primary takes the permitted stack; secondary takes the measured half. Test odd counts and single items. |
| Carried stack + empty/compatible slot | Primary places up to capacity; secondary places one. Respect limits and retain leftovers. |
| Carried stack + incompatible/restricted slot | Swap only when admitted; otherwise no mutation. Empty slots remain navigable even if this carried item cannot be placed there. |
| Auto-place | Reuse current quick-move routing; verify player↔storage, equipment/offhand and machine-specific destinations. Do not equate every auto-place action to Shift-click without evidence. |
| Gather and drag distribution | Only where mappings/capabilities allow. Double-press timing, hold-place-one and distribute-across-focus semantics require captures; do not import mouse timings as controller facts. |
| Drop | Explicit one/all actions, contextual prompts and current ownership. Navigation through blank space must never itself drop a stack. Verify off-panel activation separately from explicit Drop. |
| Crafting output | Output-only slot; output primary/secondary/tertiary may differ from ordinary pickup. Reuse craft-once/all/placement admission and stop when ingredients/capacity/credits fail. |
| Recipe list | Select/fill, craft and craft-all only as resolved/witnessed. Ghost previews are not movable inventory. Tabs/filter/page/search are non-slot actions. |
| Furnace family | Distinct input/fuel/output roles; no placing into output. Validate quick-transfer routing, output collection, progress updates and recipe-book controls. No accidental repeated output request after a server update. |
| Close/back | Reuse existing cursor/crafting cleanup and generation handling. Refused/queued cleanup must not silently erase carried items or duplicate close requests. |

Held actions are not automatically repeatable. A capability descriptor must
declare press/release/hold/repeat behavior and whether revalidation is required.
Queue pressure/rejection must not publish partial moves, fabricate success or
retry indefinitely. Preserve sparse prediction, request ordering, stack identities,
server replies and recovery. Navigation remains responsive while a transaction
is pending; prompts distinguish unavailable actions from pending authority.

### 5.4 Feedback and configuration

Show a single clear selected/hovered target, tooltip and carried stack at the
same visual cursor used for dispatch. Contextual hints should reflect the target,
carried stack, permissions and settings—not a static "A Select" footer everywhere.
Use physical-position bindings plus controller-family glyphs and existing swap
options. Remapping changes prompts and behavior together. Unknown hardware gets
accurate generic glyphs, not an assumed Xbox label.

Add separate controller cursor sensitivity/hide-cursor behavior through the
existing settings owner; retain visible focus when the cursor is hidden.
Text fields need an intentional entry/exit path and IME/keyboard coexistence.
A full on-screen keyboard is a separately scoped dependency, not silently included
in slot navigation. Optional navigation sound/haptics must not block actions;
rumble is not required for the macOS adapter's first milestone.

## 6. Screen acceptance matrix

The first vertical slice is survival inventory → chest → crafting table → furnace.
Do not call it complete coverage of these later stations:

| Family | Required controller witnesses |
| --- | --- |
| Survival inventory | Main grid/hotbar, armor/offhand, 2×2 inputs/output, book open/closed, recipe tabs/search, carried close and full-inventory overflow. |
| Storage | Single/double chest, barrel, ender chest, shulker and hopper; player↔storage traversal, split/merge/quick move and server close. |
| Crafting table | 3×3 cells, recipe selection/fill/filter, missing ingredients, craft once/all, output capacity, close cleanup. |
| Furnace/blast furnace/smoker | Input/fuel/output, valid/invalid fuel/input, dual-purpose items, running progress, partial output/XP and recipe list. |
| Creative | Category/group/search navigation and scroll, take versus ordinary move, hotbar delete/clear only under the corresponding setting. |
| Other stations | Anvil name/cost/output, enchanting choices, loom/stonecutter scrolling, cartography, smithing, grindstone, brewing, beacon, crafter toggles and trading. Each keeps its own transaction/capability rules. |
| Other item-bearing screens | Mount inventories, lectern/books and server-restyled containers: explicit reachability/ownership decisions, not an automatic generic-grid claim. |

Every accepted screen needs narrow/wide window, GUI scale and Retina/DPI checks,
keyboard/mouse↔controller transitions, reconnect, drift, held-direction repeat,
modal interruption and invalidated target tests. Server-managed screens need both
accepted and rejected inventory replies. Functional coverage and native parity
are separate verdicts.

## 7. Implementation ownership and contracts

Fit changes into existing crate directions in `tools/architecture/policy.toml`;
these are proposed owners, not permission to add a cycle or Bevy to domain crates.

| Owner | Responsibility |
| --- | --- |
| `crates/input` | Contextual UI bindings/normalized intent if supported by its existing ownership; no presentation geometry or OS APIs. |
| `crates/json-ui` | Authored focus/magnet/mapping interpretation; extend existing algorithms rather than creating another graph engine. |
| `crates/ui-screens` / `crates/ui-engine` | Current laid-out target geometry, semantic hit association and scroll/reveal integration. Keep semantic inventory targets out of lower-level generic JSON parsing. |
| `crates/client-ui` | Bevy-free per-screen controller navigation/cursor/gesture state, contextual action dispatch and prompt model. Existing inventory ledger remains authority. |
| `crates/inventory` | Only genuinely missing domain operations; all controller mutations pass through existing admission/prediction/request machinery. |
| `app/src/ui_runtime` | Bevy event sampling, owner arbitration and ordered dispatch/render integration. No OS cursor injection requirement. |
| Launcher/client presentation | Persisted UI options and rendered cursor/focus/glyphs; no independent action-binding table. |

Suggested shared contract: frame input carries controller identity, screen/window
generation, ordered edges/axes and caller time; navigation output identifies a
stable semantic target plus current geometry revision; dispatch returns admitted,
not applicable or rejected with a reason. Use existing equivalents wherever they
already exist. Do not create duplicative facades just to match these names.

## 8. Delegation-ready work packages

**C0 — Reference capture and binding contract (first; blocks parity defaults).**
Record target Bedrock build/platform, Classic/Pocket profile, controller model,
physical layout/swaps and displayed hints. Capture cursor stick roles, D-pad,
initial focus/restore, panel edges, repeat, magnet behavior, primary/secondary/
auto-place/drop/gather, crafting and furnace behavior. Record timings in units of
time, not an inferred number of rendered frames. Compare before/after item counts
and server outcomes where available. Output a compact evidence table with
confirmed defaults and explicit unknowns. No implementation agent invents a gap.

**C1 — Target geometry and authored navigation audit (parallel with C0).**
Own `json-ui` / `ui-screens` integration. Prove inventory hit regions retain the
required focus/magnet metadata and semantic addresses under clipping/scrolling.
Add pure fixtures for overrides, no-wrap, panel transitions and target invalidation.
Deliver the narrow target/reveal contract before C2/C3 edit shared state.

**C2 — Cursor/focus state reducer (after C0 contract + C1).**
Own the new Bevy-free `client-ui` module. Implement directional focus, virtual
cursor, meaningful-input mode switching, deterministic repeat, active-controller
ownership and lifetime resets. Fake clock tests; no OS APIs or ledger mutations.

**C3 — Inventory actions and prompts (after C0 contract; parallel with C2).**
Own contextual action/capability dispatch in `client-ui`, with the existing ledger.
Start with ordinary slots/chest, then crafting/furnace. Add tests for split/merge,
output-only restrictions, auto-place destinations, queue refusal/rejection and
carried close. Prompts must be derived from the same dispatch capabilities.

**C4 — Bevy/UI integration and settings (after C2/C3).**
Own app routing and existing settings/presentation adapters. Preserve event order,
one owner, gameplay suppression, glyph swaps and independent cursor sensitivity.
First acceptance: controller-only survival→chest→crafting table→furnace, without
touching the physical mouse. Do not change the hardware adapter or camera curve.

**C5 — Extended screens and acceptance (after the first vertical slice).**
Delegate stations in non-overlapping groups, each with explicit capability and
reference evidence. Capture native versus Cinnabar results and update this matrix.
Use client MCP where appropriate, plus real hardware for connection/analog issues.

Each subagent gets the baseline revision, this spec, exact owned files, agreed
contracts, named test cases and prohibited assumptions. Separate worktrees for
concurrent edits. No two agents independently own `ScreenState` or the app's input
dispatch. Follow the build limiter/workflow; no parallel full-workspace builds.

## 9. Minimum regression and completion gates

- Deterministic fake-clock tests for first press, repeat, diagonal/opposite input,
  release, drift and mode transitions; outcomes independent of 30/60/144 Hz.
- Geometry tests for authored routes/no-wrap, disabled/clipped targets, partial
  rows, virtualized lists, resize and stale recipe/slot identities.
- Semantic action traces verify **exactly one** admitted operation per intended
  edge, including simultaneous mouse/controller events and screen changes.
- Ledger tests cover counts 1/odd/full, stack capacity/restrictions, full
  destinations, pending/rejected requests, server close and cleanup ordering.
- Integration tests prove UI focus blocks camera, movement, world drop and
  gameplay-bound buttons; closing does not leak the same edge into the world.
- Real controller-only capture of the first vertical slice, with cursor/highlight,
  tooltip/carried-stack placement and contextual glyphs inspected visually.
- No unsupported "all containers" or "Bedrock parity" claim. Record each missing
  station, unmeasured timing/curve and native-evidence gap in `plan.md`.

No Rust builds or implementation changes are required to review this specification.

## References

1. Official UI source, pinned by the project:
   [Mojang bedrock-samples v1.26.50.4](https://github.com/Mojang/bedrock-samples/tree/46ba6ea985fb5a92d79a9419198f10dda14c199d/resource_pack/ui).
   Inspected `ui_common.json`, `inventory_screen.json`, `furnace_screen.json`,
   `inventory_screen_pocket.json` mappings/search results, and
   `settings_sections/controls_section.json`. Assets remain outside this checkout;
   no Mojang asset JSON is copied into this spec or proposed commits.
2. [Minecraft Wiki: Playing with a controller](https://minecraft.wiki/w/Tutorial:Playing_with_a_controller),
   "How to use the controller": native Bedrock support, GUI cursor and sensitivity.
   [Controls § Controller](https://minecraft.wiki/w/Controls#Controller) documents
   gameplay controls but is **not** a full container-binding specification.
3. [Controlify source at the inspected revision](https://github.com/isXander/Controlify/tree/68dba946a2732c0ab2112fddbc3abe05a491a565),
   paths in §4, plus `docs/content/reference/builtin-bindings.md` and `LICENSE`.
   [Public documentation](https://controlify.isxander.dev/) is an overview, not a
   substitute for the pinned implementation or Bedrock capture.
4. Existing Cinnabar behavior references:
   [normal transactions](inventory-normal-transactions.md),
   [sparse prediction](inventory-sparse-prediction.md),
   [crafting and cursor cleanup](inventory-crafting-close.md),
   [geometry](inventory-gui-geometry.md),
   [recipe admission](inventory-recipe-admission.md).
