# Packet reactor panel and input capabilities

A core packet reactor can import the following functions, independently of native
camera exports. Every call requires `controls`. The host does not negotiate a menu
bit, call a menu export, choose an opening key, or reopen retained content.

| Import module | Function | Core signature |
| --- | --- | --- |
| `cinnabar:extension/panel` | `set-content` | `(i32 pointer, i32 length) -> i32` |
| `cinnabar:extension/panel` | `update-content` | `(i32 pointer, i32 length) -> i32` |
| `cinnabar:extension/panel` | `open` | `() -> i32` |
| `cinnabar:extension/panel` | `close` | `() -> i32` |
| `cinnabar:extension/panel` | `toggle` | `() -> i32` |
| `cinnabar:extension/input` | `read` | `(i32 pointer, i32 capacity) -> i32` |

Content calls read UTF-8 JSON from exported linear `memory`. `set-content` uses the
same `Panel` schema and validation as frame mods; empty content removes it.
`update-content` uses the frame panel patch schema against retained content.
Writes return zero on success. All calls return `-1` when controls are denied,
`-2` for invalid request data, and panel operations return `-3` when content is
required but absent. Rejected requests preserve retained state and let packet
processing continue. Opening requires content. Open and close are idempotent; toggle flips the current
requested visibility, including earlier calls.
Invalid memory ranges trap the current call and quarantine the reactor under the
packet runtime's existing failure policy. Actor projections require observation capabilities and are not admitted
by these surface-only calls. Panel bounds come from `ui::mod_panel`.

`read` returns the UTF-8 JSON byte count. When capacity is smaller, it writes
nothing and retains pending events; query with capacity zero, allocate, then read
again. If input grows between calls, retry with the returned larger size. A successful copy consumes press, control and field events. Held state
remains available. The JSON object has:

- `open`: whether this owner currently owns the open panel; `focused`: whether
  the sample has physical focus without another screen absorbing it.
- `viewport`: optional authored GUI dimensions.
- `held`: physical key and mouse button names; `keys`: press records with `key`
  and optional composed `text`. These use the frame input physical names and bounds.
- `pointer`: optional GUI coordinates while the panel owns focused input; `wheel`:
  wheel notches, positive away from the player.
- `pointer_held`, `pointer_cancelled`, `shift_held`: raw physical state and pointer
  interruption, with no resolved gameplay actions.
- `events`: numeric `{id,value}` control events from the declared panel.
- `fields`: shared panel field events (`Change`, `Submit`, `Key`) serialized using
  the panel schema, including field IDs, values and physical modifiers.

Input is available while focused, including keys while the panel is closed.
Other focused screens absorb gameplay input. Press and control queues retain a
bounded prefix until read; held state and pointer samples reflect the latest frame.
The guest reads input from its ordinary packet callbacks or tick and explicitly
calls `open`, `close` or `toggle` according to its own policy. It must declare the controls request
in its grant metadata. No input subscription claims an opening key for the host.
These raw core imports do not change the component `packet-mod` world.

The latest explicit open request owns the panel and closes other owners. Escape
closes an open panel unless `capture_key` is set; native text editing, clipboard
shortcuts and IME remain standard UI navigation. The host handles no opening key.
The removed `toggle_key` field is rejected. Remove it from panel content and
replace `set-open(bool)` imports with `open()` or `close()`.

The removed `menu(i32,i32)->i32` export and capability bit 2 have no replacement
menu loop. Native camera frames retain bit 1 and their existing contract.
