# Settings and storage

A personal mod has exactly one place to persist data: the `settings` interface, backed by a companion file that belongs to your component. There is no filesystem access, and no other storage.

```rust
use mod_api::bindings::cinnabar::extension::settings;

let stored = settings::load(); // Result<String, String>
let saved = settings::save(r#"{"version":1}"#); // Result<(), String>
```

Both functions need the `settings` grant; without it they return an error. See [Grants](grants.md).

## The companion file

The companion is a fixed file beside the selected component — `settings_companion` resolves it as the component path with its extension replaced by `settings.json`. For a directory package, it is `<directory name>.settings.json` beside that directory. You never name or open it; the host reads it before your mod starts and hands its contents to `settings::load`, and writes what you save. Moving or renaming a component changes that path; move the companion separately if you want to keep its settings. The registration list rejects duplicate component paths and active mods sharing a settings companion.

Rules the host enforces on `save`:

- The payload must be a JSON object — not an array, not a bare value.
- It is at most `MAX_SETTINGS_BYTES` (16 KiB).
- The write is staged during the callback and queued only after the callback succeeds: a callback that traps saves nothing from that callback. The background writer replaces the companion atomically; a successful `save` does not mean the disk write has finished.

`load` returns the stored string as-is, or an empty string when nothing has been saved. Parsing and migrating it is your job.

## Writing settings that survive your own updates

Treat the loaded string as untrusted input from an older version of your mod:

- Parse defensively. An empty or malformed string means "use defaults", not "trap" — a trap quarantines the mod, as [How mods run](how-mods-run.md) describes.
- Carry a version field and reject or migrate versions you do not understand. Supply defaults for missing fields and fall back to a clean state when a version is unsupported.
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

Separately from settings, the `assets` interface (`cinnabar:world-draw/assets`) reads immutable files supplied by the host: `read`, `length`, and `read-range` for streaming a large file across callbacks. It requires `world_draw`; reads share the drawing call and byte budgets. The paired client-part host supplies its verified bundle assets, but the personal-component and directory-package loaders currently supply no files to this interface: even with the grant, their reads return an unowned-asset error. Assets are for data your mod ships, not for anything the mod writes. See the [assets reference](../reference/world-draw-assets.md).
