# How mods run

## Callbacks

A personal mod exports two required callbacks:

- `init` runs once when the component starts.
- `frame` runs once per frame.

In Rust you implement the generated `Guest` trait and export it, as in [Getting started](getting-started.md). Everything your mod does happens inside a callback: you read observations and input, then publish output. Output is committed only after the callback returns successfully. If a callback traps, everything it staged in that callback is discarded, so the game never sees half of a frame's work.

Callbacks are also transactional across a session. Joining, disconnecting or changing connections creates a fresh guest, so observations cannot carry across a connection boundary. A dimension transfer on the same connection keeps guest memory and HUD surfaces, but world drawing handles and pending world output are cleared: rebuild world drawing after a transfer, as [Camera and rendering](camera-and-rendering.md) describes.

### The `player-mod` events

A mod targeting the `player-mod` world gets optional event callbacks on top of `init` and `frame`: `screen-changed`, `action`, `secondary-action`, `scrolled`, `text-changed`, `key`, `data-changed` and `view-closed`. Implement the `PlayerMod` trait from `mod_api` and call `export_player_mod!`; every event defaults to doing nothing, and the host only calls the ones your component exports. New events arrive as new optional exports, so a built component keeps working when the API grows.

Input edges expire, while the latest layout and session changes wait for a callback lane. That is, a key press is a fact about one frame, but a screen layout change is delivered when there is budget to deliver it rather than dropped.

## The fuel budget

Guest execution is metered in fuel, roughly WebAssembly instructions. Each callback kind has its own allowance:

| Callback | Fuel |
| --- | --- |
| `frame` | 200,000 (`FRAME_FUEL` in `mod-host`) |
| Other event callbacks | 10,000,000 (`CALLBACK_FUEL`) |
| `data-changed`, and `init` of a component that exports it | 100,000,000 (`LOAD_FUEL`) |
| Any other `init` | Shares the frame allowance |

All loaded mods share a per-frame pool of 400,000 fuel (`mod_api::TOTAL_MOD_FRAME_FUEL`) for their frame callbacks. When the pool cannot pay a callback's full allowance, the callback is deferred to a later frame rather than run with a smaller limit and trapped partway. Deferred frame callbacks keep the state that depends on them: terrain subscriptions, touch taps and text input batches are held for the callback that eventually runs.

Per-callback caps sit under the fuel budget too. Imports count calls — for example a callback may make at most 8 presentation calls, 128 `facts` reads, or 32 render and retained-drawing calls — and one call over the cap traps the callback. The per-interface guides and reference pages give each cap.

### Spreading work with `budget.remaining-fuel`

Heavy one-off work — indexing a catalogue, encoding a large settings document — rarely fits the frame budget alongside your per-frame work. The `budget` interface reports the running callback's own remaining fuel:

```rust
use mod_api::bindings::cinnabar::extension::budget;

if budget::remaining_fuel() > 3_000 {
    // Cheap enough to do now; otherwise leave it for the next callback.
}
```

It needs no grant, because it reads no game state. The pattern the first-party Cinnabar Client mod uses in its `fuel.rs` is worth copying: record `remaining_fuel()` at the start of the callback, keep a target below the 200,000 trap line, and before each piece of optional work check that what is left still covers the work that must follow it. Work you skip stays queued in your own state for the next callback. Measuring actual fuel beats guessing a fixed step count, because the same code costs different fuel depending on what the host imports did before it.

Guest memory is capped at 16 MiB for personal mods.

## When a mod traps or overruns

A frame callback that runs out of fuel is treated differently from every other failure, because a single expensive frame is often transient:

1. The host retains the mod's committed output and marks the instance as recovering; the trapped component is never entered again.
2. It restarts the mod in a fresh store — guest memory resets, `init` runs again — on the loading budget.
3. If the mod overruns the frame budget three times in a row (`MAX_CONSECUTIVE_FRAME_OVERRUNS`), or the restart itself fails, the mod is quarantined.

Any other trap — a panic, a memory fault, exceeding a per-callback call cap — quarantines the mod straight away. Quarantine clears that mod's outputs: labels, surfaces, annotations, drawing, retained state. Other mods are unaffected.

A quarantined mod recovers by reloading a valid changed component, from Settings → Mods or by replacing the file and reloading as in [Getting started](getting-started.md). Permission changes also reload the mod; revoking a grant never turns an existing quarantine into a recoverable state — the quarantine stands until a reload succeeds.

Write defensively with this in mind: check the `Result` a host import returns instead of assuming it succeeded, because a denied or failed call returns an error you can react to, while a panic costs you the whole instance.
