# How mods run

## Callbacks

A personal mod exports two required callbacks:

- `init` runs once when the component starts.
- `frame` starts when admitted and may resume across several game frames.

In Rust you implement the generated `Guest` trait and export it, as in [Getting started](getting-started.md). Everything your mod does happens inside a callback: you read observations and input, then publish output. Output is committed only after the callback and its post-return cleanup finish successfully. If a callback traps, everything it staged in that callback is discarded, so the game never sees half of a frame's work.

Joining, disconnecting or changing connections requires a fresh guest, so observations cannot carry across a connection boundary. A dimension transfer on the same connection keeps completed guest memory and HUD surfaces, but world drawing handles, native presentation policies, environment overrides and pending world output are cleared: rebuild world output after a transfer, as [Camera and rendering](camera-and-rendering.md) describes.

### The `player-mod` events

A mod targeting the `player-mod` world gets optional event callbacks on top of `init` and `frame`: `screen-changed`, `action`, `secondary-action`, `scrolled`, `text-changed`, `key`, `data-changed` and `view-closed`. Implement the `PlayerMod` trait from `mod_api` and call `export_player_mod!`; every event defaults to doing nothing, and the host only calls the ones your component exports. New events arrive as new optional exports, so a built component keeps working when the API grows.

Input edges expire, while the latest layout and session changes wait for a callback lane. That is, a key press is a fact about one frame, but a screen layout change is delivered when there is budget to deliver it rather than dropped.

## Suspension and publication

Frame callbacks without decision exports run cooperatively. The host checks elapsed time between roughly 25,000-fuel slices, sharing a soft target of about 1 ms across runnable mods. Mandatory synchronous callbacks count against that total. Owners receive equal shares, carry overshoot debt across frames, and pass unused shares to other runnable owners. A host import or one fuel slice can overshoot the target.

`Deferred` means the invocation has not started. `Suspended` means the same invocation will resume with its guest memory, remaining fuel, import counters and private output transaction intact. A self-wake cannot poll an expired slice again in the same game frame. There is no suspension timeout or count that becomes a fault. Fuel is the only execution fault limit for these callbacks; invalid imports, malformed output and guest traps still fail normally.

Input, session data, local time and elapsed time are frozen when the invocation starts. Changes observed while it is suspended belong to a later invocation. Reads can see the invocation's staged writes, including saved settings, without publishing them. The last completed HUD, panel and drawing remain visible while work is suspended. Frame-only movement and camera overrides expire normally.

A callback completed after its starting frame can publish retained presentation and settings. Its movement, interaction, jump and use pulses, commands, cues, inventory requests, packet-delay changes, pointer placement, panel-open requests and text-focus edge are dropped. Publication checks current grants, session, input ownership and screen identity. A changed screen or native edit prevents stale UI output from replacing the current UI.

The host never re-enters a suspended instance for an event. It coalesces lifecycle changes and keeps at most the latest text per declared control, with separately bounded native edit queues. Click, key, scroll and touch edges received during suspension expire. World or session changes, grant revocation and unload cancel the continuation without a fault. The partial guest is discarded and, when still authorized, rebuilt through the loading lane. A failed hot reload keeps the old continuation; a successful replacement cancels it atomically.

Action filters, sound handlers and teleport handlers need immediate synchronous decisions and share guest state with `frame`. Any instance exporting one of them keeps synchronous frame execution and its existing deadlines, even when that handler lacks a grant. Events and loading also remain synchronous at the host boundary. Standalone host callers without a shared schedule drive async calls to completion.

## The fuel budget

Guest execution is metered in fuel, roughly WebAssembly instructions. Each callback kind has its own allowance:

| Callback | Fuel |
| --- | --- |
| `frame` | 200,000 (`FRAME_FUEL` in `mod-host`) |
| Other event callbacks | 10,000,000 (`CALLBACK_FUEL`) |
| `data-changed`, and `init` of a component that exports it | 100,000,000 (`LOAD_FUEL`) |
| Any other `init` | Shares the frame allowance |

All loaded mods share a per-frame pool of 400,000 fuel (`mod_api::TOTAL_MOD_FRAME_FUEL`) for their frame callbacks. When the pool cannot pay a callback's full allowance, the callback is deferred to a later frame rather than run with a smaller limit and trapped partway. Deferral preserves terrain subscriptions, touch taps and text input batches until an invocation starts. Suspended invocations retain their original full fuel limit without refilling it. Each serviced frame conservatively reserves an ordinary callback share from the pool; unused fuel is refunded only when the completed invocation reports its cumulative consumption.

Per-callback caps sit under the fuel budget too. Imports count calls — for example a callback may make at most 8 presentation calls, 128 `facts` reads, or 32 render and retained-drawing calls — and one call over the cap traps the callback. The per-interface guides and reference pages give each cap.

### Spreading work with `budget.remaining-fuel`

Heavy one-off work — indexing a catalogue, encoding a large settings document — rarely fits the frame budget alongside your per-frame work. The `budget` interface reports the running callback's own remaining fuel:

```rust
use mod_api::bindings::cinnabar::extension::budget;

if budget::remaining_fuel() > 3_000 {
    // Cheap enough to do now; otherwise leave it for the next callback.
}
```

It needs no grant, because it reads no game state. Record `remaining_fuel()` at the start of the callback, keep a target below the 200,000 frame trap line, and before each piece of optional work check that what is left still covers the work that must follow it. Work you skip stays queued in your own state for the next callback. Measuring remaining fuel beats guessing a fixed step count, because earlier work in the callback leaves less fuel for the next piece.

Guest memory is capped at 16 MiB for personal mods.

## When a mod traps or overruns

A frame callback that runs out of fuel is treated differently from every other failure, because a single expensive frame is often transient:

1. The host retains the mod's committed output and marks the instance as recovering; the trapped component is never entered again.
2. It reserves the shared loading lane and restarts the mod in a fresh store — guest memory resets and `init` runs again. The usual `init` fuel rule still applies: only a component exporting `data-changed` gets `LOAD_FUEL` for `init`.
3. If the mod overruns the frame budget three times in a row (`MAX_CONSECUTIVE_FRAME_OVERRUNS`), or the restart itself fails, the mod is quarantined.

Any other trap — a panic, a memory fault, exceeding a per-callback call cap — quarantines the mod straight away. Quarantine clears that mod's outputs: labels, surfaces, annotations, drawing, retained state. Other mods are unaffected.

A quarantined mod can be restarted with Reload in Settings → Mods, even if the component bytes have not changed. Fix a persistent trap before restarting it. Development components selected through the environment are watched for changed bytes; an invalid replacement keeps the old instance. Permission changes also prepare a replacement under the new grants; revocation alone does not reactivate a quarantined instance.

Write defensively with this in mind: check the `Result` a host import returns instead of assuming it succeeded, because a denied or failed call returns an error you can react to, while a panic costs you the whole instance.
