# Settings and storage

A mod has exactly one place to persist data: the `settings` interface, backed by a companion file that belongs to your component. There is no filesystem access, and no other storage.

```rust
use mod_api::bindings::cinnabar::extension::settings;

let stored = settings::load();          // Result<String, String>
settings::save(&json)?;                  // stages a JSON object for persistence
```

Both functions need the `settings` grant; without it they return an error. See [Grants](grants.md).

## The companion file

The companion is a fixed file beside the selected component — `settings_companion` resolves it as the component path with its extension replaced by `settings.json`. You never name or open it; the host reads it before your mod starts and hands its contents to `settings::load`, and writes what you save. Because the path is derived from the component, two registrations of the same component share one companion, and moving the component moves its settings with it.

Rules the host enforces on `save`:

- The payload must be a JSON object — not an array, not a bare value.
- It is at most `MAX_SETTINGS_BYTES` (16 KiB).
- The write is staged during the callback and persisted only after the callback succeeds, atomically: a callback that traps saves nothing.

`load` returns the stored string as-is, or an empty string when nothing has been saved. Parsing and migrating it is your job.

## Writing settings that survive your own updates

Treat the loaded string as untrusted input from an older version of your mod:

- Parse defensively. An empty or malformed string means "use defaults", not "trap" — a trap quarantines the mod, as [How mods run](how-mods-run.md) describes.
- Carry a version field and reject or migrate versions you do not understand. The first-party Cinnabar Client mod does this in its `persistence.rs`: a `version` number, defaults for missing fields, and an error for an unsupported version that falls back to a clean state.
- Encode incrementally if your document is large. Serializing a big settings tree in one frame callback can overrun the fuel budget; check `budget::remaining-fuel` and advance the encoding over several callbacks, the pattern described in [How mods run](how-mods-run.md).

## What survives what

| Event | Guest memory | Settings |
| --- | --- | --- |
| Reload with a valid component | Reset (`init` runs again) | Preserved |
| Reload with an invalid component | Previous instance keeps running | Preserved |
| Connection change | Fresh guest | Preserved |
| Dimension transfer, same connection | Kept | Preserved |
| Quarantine | Instance stopped, outputs cleared | Last successful save stands |

Load settings in `init`, keep the working copy in your own state, and save when something changes rather than every frame. Saving stages a write on a single coalescing persistence lane, so frequent saves are merged — but each one still has to fit the callback that stages it.

## Package files are read-only

Separately from settings, the `assets` interface (`cinnabar:world-draw/assets`) lets a mod read immutable files inside its own package, verified by the host: `read`, `length`, and `read-range` for streaming a large file across callbacks. A missing or unowned path is refused, and reads share the callback's byte budget. Assets are for data your mod ships, not for anything the mod writes. See the [assets reference](../reference/assets.md).
