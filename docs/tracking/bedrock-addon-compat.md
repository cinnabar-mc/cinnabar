# Bedrock add-on compatibility: held items, animations and HUD

## Task

Investigate and extend generic Bedrock resource-pack compatibility using TACZ as
an end-to-end reproduction. Identify the causes before selecting implementation
changes. Do not introduce item-identifier checks, add-on-specific rendering paths,
or a host-provided weapon HUD.

Status: initial blocking causes identified; implementation and visual acceptance
remain pending. No implementation fix is claimed.

## Reported behavior

With TACZ active on a Bedrock Dedicated Server, the official Minecraft client
shows the held weapon and ammunition HUD. The reported Cinnabar behavior is:

- A gray square replaces the held weapon.
- Shooting and reload animations are absent.
- The ammunition count and ammunition type are absent.

The reference screenshot establishes a visible model and ammunition HUD, not
animation behavior. The animation comparison is reported by the user. Treat the
three symptoms independently until their data paths establish a shared cause.

## Reproduction contract

Use the same server, active pack versions and item state for comparisons. Record
the server build, protocol, client build and pack identity in local evidence.
Do not publish account details, server addresses, game assets or packet captures.

1. Join the authorized BDS with the required resource packs accepted.
2. Obtain a known weapon and its ammunition through normal supported controls.
3. Select its server-owned hotbar slot and confirm authoritative equipment state.
4. Capture the held model and ammunition HUD while idle.
5. Fire and capture the animation and ammunition change.
6. Reload and capture its animation and updated ammunition state.
7. Switch away and back; check that presentation and state remain consistent.

Use the developer-control MCP path with `headless: true`. This client's hidden
window renders frames; inspect actual captures, not only connection or draw-count
success. Keep any packet capture and logs outside tracked source files. Do not
restart or reconfigure the server, modify the installed add-on, or edit world
saves as part of this investigation.

## Investigation

### Resources and held-item rendering

- Establish the exact active resource-pack stack and compare it with the source
  archive before treating archive contents as the runtime input.
- Trace item identity and icon selection separately from held geometry.
- Follow pack resolution, textures, attachables, materials, render controllers and
  the first-person draw path to the first observable divergence.
- Distinguish missing resources, parse failures, unsupported semantics and a
  deliberately selected fallback.

### Animation and state

- Determine the add-on's actual shooting and reload mechanisms from its data.
- Trace physical input, outbound actions, server updates, player/attachable
  animation context, Molang queries and variables, controllers and final pose.
- Inspect explicit animation packets if the add-on uses them; do not assume it
  does, or that receiving such a packet proves an animation was evaluated.

### Ammunition HUD

- Identify the actual producer and presentation mechanism: title/actionbar,
  JSON-UI, text, font glyphs or another authored resource path.
- Trace delivery, binding updates, resource overrides and visible output.
- Keep server-provided content distinct from Cinnabar's own first-party mod API.

## Implementation proposal requirements

Each proposal must name:

- A confirmed discrepancy, its source/data references and evidence boundary.
- The owning crate/module and the generic Bedrock behavior to implement.
- A minimal regression fixture and an assertion that fails before the fix.
- Dependencies on other proposals and any remaining native-reference question.
- A focused verification command and live visual acceptance scenario.

Prefer the lowest existing owner of the data, preserve crate boundaries, and
follow existing diagnostic and error-handling conventions. Keep changes
independent where their causes are independent. Do not compile broad workspace
test suites during parallel read-only analysis.

## Acceptance criteria

- [ ] A repeatable baseline reproduces each reported symptom, or its absence is
      explicitly recorded with the tested build and state.
- [ ] Each cause is linked to a specific resource/state-to-consumer discrepancy;
      static findings are not described as live runtime proof.
- [ ] Proposed changes support the underlying Bedrock mechanism rather than TACZ
      identifiers or hard-coded weapon behavior.
- [x] Minimal regression tests fail before each implementation change and pass
      afterward.
- [ ] The weapon model renders with the expected texture and placement.
- [ ] Shooting and reloading produce the authored animation behavior.
- [ ] Ammunition count and type render and follow authoritative state updates.
- [ ] Vanilla held items and independent generic add-on fixtures remain correct.
- [ ] Real before/after rendered frames cover the visual changes; recordings or
      sampled animation frames cover animation behavior.
- [x] Remaining parity work is reflected in the existing relevant `plan.md`
      section and completion ledger when implementation changes land.

## Confirmed compatibility gaps and proposed changes

### Large player definitions are discarded before rig compilation

A Bedrock-accepted player definition in the reproduction has 664 animation
aliases and 859 unique typed dependencies. `collect_string_leaves` rejects the
513th string before deduplication, and per-file preparation classifies the whole
entity as unparsable. The player symbol, geometry aliases and animation roots are
lost together. A minimal fixture referencing the same clip through 512 aliases
is accepted; 513 aliases discard the definition.

Change the input traversal and dependency accounting in
`crates/pack-compiler/entity/src/entity.rs`, coordinated with catalog validation
in `crates/assets/src/entity.rs` and carrier preflight in
`crates/assets/src/entity/v4/preflight.rs`. Separate raw alias counts from unique
references and replace unverified per-entity limits with checked indexing and
appropriate aggregate resource accounting. Review consumers of
`MAX_ENTITY_DEPENDENCIES`, including particle scans; do not silently truncate
valid content or merely substitute a larger arbitrary constant.

Test many aliases of one clip, a large unique dependency set, carrier
encode/decode, and isolation of an independently invalid source. Admission of the
player definition is a prerequisite, not proof that every downstream animation
and first-person layer is correct.

### Built-in HUD placement leaks into server-authored controls

The built-in Java HUD gives `hud.hud_actionbar_text` bottom-to-top anchors. A
server overlay replaces its type and content while relying on vanilla's default
anchors. Ordinary property-wise merging retains the Java anchors and places the
nested label below the viewport. The text is delivered and resolved, but clipped.

A production JSON-UI layout probe at a 640-by-360 virtual viewport emits one text
draw with vanilla plus the server overlay, zero with the Java HUD underneath,
and one after restoring only the vanilla actionbar definition before the server
overlay. This isolates placement from text delivery, alpha and font handling.

Extend the existing restore-before-server-layer policy in
`crates/ui-screens/src/hud.rs` and `crates/ui-screens/src/hud/titles.rs`, or
otherwise isolate Cinnabar-owned presentation defaults from server-owned
controls. Preserve normal Bedrock merge semantics in `crates/json-ui/src/pack.rs`.
Use control ownership, not add-on identifiers, to select the policy.

Test a server panel that omits anchors, multiline and changing text, and the
unchanged built-in HUD when no server override exists. Inspect real frames after
implementation.

### Rejections need source-level attribution

`crates/pack-compiler/entity/src/entity/pack/prepared.rs` currently loses the
specific parser/validation reason through `.ok()?`. Retain a bounded diagnostic
with the source path, failing stage and reason, so a resource-budget rejection is
not reported only as an unparsable file. Keep bad-file isolation and avoid
per-frame logging.

These are independent, generic changes. The reproduction also contains an
invalid geometry document for a different weapon; do not treat editing that
add-on file as the AKM compatibility fix or broadly relax JSON parsing without
native-behavior evidence.

## Evidence policy

Keep environment-specific investigations, agent reports, logs, captures and
credentials out of this public checkout's tracked files. This document is the
contributor-facing task specification, not a chronological investigation diary.
The absence of private maintainer instructions does not justify inventing their
contents or claiming maintainer setup is complete.
