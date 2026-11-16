# Bedrock add-on compatibility: held items, animations and HUD

## Task

Investigate and extend generic Bedrock resource-pack compatibility using TACZ as
an end-to-end reproduction. Identify the causes before selecting implementation
changes. Do not introduce item-identifier checks, add-on-specific rendering paths,
or a host-provided weapon HUD.

Status: generic dependency-admission and actionbar-layering fixes are implemented
and covered by passing regression tests. A headless BDS retest confirms visible,
updating ammunition count/type, but the held weapon still appears as a gray plane;
authored gun animation acceptance remains incomplete. Flat Molang chain handling
now passes compiler/evaluator regressions and retains the complete pre-animation
program in an isolated pack probe. Additional first-person player-controller
models and live animated rendering remain active work.

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
aliases and 859 unique typed dependencies. The baseline `collect_string_leaves`
rejected the 513th string before deduplication, and per-file preparation classified
the whole entity as unparsable. The player symbol, geometry aliases and animation
roots were lost together. A minimal fixture referencing the same clip through
512 aliases was accepted; 513 aliases discarded the definition.

Dependency traversal now streams leaves and deduplicates references. The shared
`EntityDependencyStorageBudget` charges retained dependency entries and owned
identifier bytes with checked arithmetic. Compiler assembly, compiled-asset
validation and streaming carrier preflight enforce the aggregate catalog budget;
preflight runs before typed dependency arrays are allocated. Existing source,
identifier, ordering and resolution checks remain. This is a retained-storage
estimate, not an exact peak-allocation ceiling during source parsing.

Regression tests exercise many aliases of one clip, a large unique dependency
set, rig/carrier round trips, an independently invalid neighboring source,
exact storage boundaries, overflow and cross-symbol preflight accounting. The
unrelated particle consumer's existing reference truncation remains explicitly
incomplete; no broader particle-limit parity is claimed. Admission is a
prerequisite, not proof that every downstream animation and first-person layer
is correct.

### Built-in HUD placement leaks into server-authored controls

The built-in Java HUD gives `hud.hud_actionbar_text` bottom-to-top anchors. A
server overlay replaces its type and content while relying on vanilla's default
anchors. Ordinary property-wise merging retains the Java anchors and places the
nested label below the viewport. The text is delivered and resolved, but clipped.

A production JSON-UI layout probe at a 640-by-360 virtual viewport emits one text
draw with vanilla plus the server overlay, zero with the Java HUD underneath,
and one after restoring only the vanilla actionbar definition before the server
overlay. This isolates placement from text delivery, alpha and font handling.

The existing restore-before-server-layer policy now restores the carrier's
actionbar definition immediately before the first server layer authoring it.
Normal JSON-UI merge semantics are unchanged, later server customizations survive,
and an untouched actionbar retains the built-in presentation. Selection uses the
vanilla control identity, not add-on identifiers.

Production-seam tests cover visible multiline/changing text, the untouched
built-in HUD, a non-authoring lower layer and multiple authoring server layers.
Headless BDS frames confirm visible ammunition count/type and delivered-value
updates. This does not qualify complete native HUD parity or gun animations.

### Flat Molang boolean chains trip the expression-tree depth bound

With vanilla reference definitions supplied, the baseline admitted the player rig
but dropped its pre-animation program. An exact-fragment probe identified a flat
logical-OR chain rejected
by the expression-tree depth bound in
`crates/pack-compiler/entity/src/entity/molang/parser.rs`. The fragment has no
parentheses or ternaries; a long flat chain becomes a deep owned expression tree.
This is an additional confirmed compiler gap, not a rendered-animation pass.

Flat AND/OR chains now use private n-ary nodes, gathered and depth-checked once
per precedence level, with iterative left-to-right short-circuit lowering and
normalized boolean results. Folding and block-state evaluation retain lazy order.
Existing source/operation budgets and genuinely nested-input guards remain; the
depth constant was not raised. Synthetic compiler/evaluator regressions cover
long dynamic chains and precedence; owning tests cover lazy folding, branch
targets, source/operation boundaries and nested-input rejection.

All exact pre-animation fragments compile, and an isolated production-library
pack probe retains the complete program. Other optional reference fallbacks remain;
this is not proof of controller selection, a rendered model or authored animations.

### Additional player-controller models are not composed into first person

`crates/client-presentation/src/actor_publication/hand.rs` publishes an arm-masked
body and held items, but does not apply the controller-layer compositor used by
`presentation/entity_layers.rs` for actor batches. The reproduction describes its
weapon with an additional first-person player-controller geometry. This is a
confirmed implementation gap; its contribution after the admission fix still
requires live evidence.

A follow-up must carry selected additional geometries, their sampled first-person
poses, materials and artwork through hand publication and the near-camera pass.
Keep normal arms, held items and map artwork independent; do not reuse a reserved
texture slot or add a weapon-specific render path. Add a regression at actual hand
publication, then qualify the rendered frame and animated sequence.

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
