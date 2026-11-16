# Bedrock add-on compatibility: held items, animations and HUD

## Task

Investigate and extend generic Bedrock resource-pack compatibility using TACZ as
an end-to-end reproduction. Identify the causes before selecting implementation
changes. Do not introduce item-identifier checks, add-on-specific rendering paths,
or a host-provided weapon HUD.

Status: investigation. No implementation fix or visual acceptance is claimed.

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

## Evidence policy

Keep environment-specific investigations, agent reports, logs, captures and
credentials out of this public checkout's tracked files. This document is the
contributor-facing task specification, not a chronological investigation diary.
The absence of private maintainer instructions does not justify inventing their
contents or claiming maintainer setup is complete.
