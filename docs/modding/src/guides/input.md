# Input

Input comes through the `input` interface. Its controls, bindings, text and field reads and key reservations need `controls`; the demo action read, `demo_pressed`, is ungated. Everything in it is local: key names, edges and events from this machine. Reading input never synthesizes gameplay input; the request functions at the end of this guide are the narrow exceptions, each with its own grant.

## Reading a frame of input

Call `input::read_controls` in `frame` to get this frame's `controls` record:

| Field | Meaning |
| --- | --- |
| `seconds` | Frame time, in seconds |
| `focused` | Whether the client window is focused |
| `gameplay` | Whether this mod received a gameplay snapshot this frame; it is false while a menu owns input and can also be false when your grants do not admit a gameplay snapshot |
| `panel_open` | Whether your mod's panel is open |
| `keys_pressed` | Physical non-repeat key edges this frame. Pointer buttons arrive as `MouseLeft`, `MouseRight`, `MouseMiddle` |
| `keys_held` | Physical keys held this frame, including your reserved ones; keys reserved by an earlier mod are withheld. Use for key capture |
| `keys_admitted` | Held physical desktop keys and modifiers the finalized native router passes to gameplay in this callback |
| `events` | Panel control events (`id`, `value`) delivered to your mod |
| `pointer` | Pointer position in HUD GUI units, only while your open panel owns input; otherwise none |
| `wheel` | Wheel notches this frame while your panel owns input; positive scrolls up |

Key names are physical key names of at most `MAX_CONTROL_KEY_BYTES` (32) bytes, alphanumeric. At most `MAX_CONTROL_KEYS` (64) keys appear in each list. Held and admitted keys use
ascending physical-name order and truncate independently at that cap. With more than 64
simultaneously held controls, action resolution from the truncated list can differ from
the host gate. The native gate still checks its complete routed sample.

When you only need part of the frame, `read_selected_controls` takes a `selection` — which `keys_pressed`, `keys_held` and `keys_admitted` names to include (none selects all, an empty list selects none) and whether to include `events` — and returns the same record with the unselected fields empty. It shares the `read_controls` callback read budget, so selecting is about copying less, not reading more often.

React to `keys_pressed` for actions. A held key appears in `keys_held` every frame, so treating held as pressed fires your action every frame; the examples check the host-assigned demo edge with `input::demo_pressed` for the same reason — it is true only for the current frame's action edge. If your mod must not act while a menu owns input, check `gameplay` and `focused`: input edges expire with their frame, and edges that arrive while your callback is deferred are not replayed later.

## Reading the player's bindings

Use `input::read_bindings(None, 0, limit)` to start reading the player's current assignments.
A reply contains a revision, a total count (at most `MAX_INPUT_BINDINGS`) and a bounded page
(at most `MAX_INPUT_BINDINGS_PER_READ` rows). Advance the offset by the page length until you
have the total; restart at zero if the revision changes. Suspend action-dependent policy
while collecting a changed revision. On later frames, pass the last complete revision to
receive `None` while unchanged. This shares the controls read allowance; small pages let
mods spread decoding and cache updates across callbacks.
Each entry names an action (for example `attack`, `jump` or `use`), a gameplay/UI context,
a physical key and required modifier groups. Alternative entries bind the same action to
several inputs. The names are the same ones in `keys_held`, including `MouseLeft`, `KeyW`
and `Space`. These are the player's assignments, including rebinds.

A mod resolves its own action holds by matching `read_bindings` chords against
`keys_admitted`. For each gameplay-context chord, require its key and all required modifier
groups in that list; either left or right satisfies a modifier group. A satisfied chord
with more required modifier groups on the same key and context takes precedence, even
when it names another action. Equal specificity remains an alternative. Collect the
complete binding revision before resolving actions.

`keys_admitted` comes from the same finalized routing sample as the host's physical
attack/use gates. It excludes macro-consumed keys, release quarantine after rebinds or
ownership changes, UI barriers, reserved keys and controls excluded by the selected input
device. A key held during a rebind can remain in `keys_held` while absent from
`keys_admitted` until released and pressed again. Use `keys_held` for key capture, including
reserved controls. Admission is sampled once for the callback; reservations requested
inside that callback affect later samples. All mods see the same native admission, while
earlier mods' reservations can also withhold raw held/pressed keys from later mods.

Within the list bounds and supported desktop coverage, chord resolution agrees with the
host's physical attack/use hold gate. A matching chord does not grant world availability,
server permission or request authority; handle refused requests. Subsequent action filtering
can still suppress a request after the ordinary frame callback.

Only keyboard keys and the three observed mouse buttons are currently covered. Gamepads,
built-in touch controls, mouse axes and additional mouse buttons are not in `keys_held`.

## Text input

Two further reads serve mods with text fields:

- `read_text_events` returns committed single-line text edits (`id`, `value`). Repeated reads in one frame callback see the same batch.
- `read_field_events` returns `field-event` values — `change` and `submit` text events, and `key` events carrying the field id, key name and modifier flags. Live values coalesce per field between frame callbacks; submit and navigation events keep their order. Preedit text stays host-local and is never delivered.

## Reserving keys

`input::reserve_keys` takes a list of physical key names to reserve before the ordinary gameplay router samples them. The reservation is retained after a successful callback. Reserve the keys your mod consumes so the game does not also act on them, and replace the reservation when your key set changes; an empty list releases it.

## Touch buttons

With `controls`, the `touch` interface lets a mod put its own buttons on the touch controls:

- `touch::set_buttons` replaces your retained buttons, at most 4. A button has an `id` (1–32 bytes of lowercase ASCII letters, digits, `_` and `-`; prefix it with your mod's name, because a player's saved placement is keyed by it and the earlier mod wins a collision), a one-line `label` (1–24 bytes, no `§` codes), a default `anchor` and `offset`, and a `size` between 18 and 96 GUI pixels per side. Players move your buttons in the touch layout editor; you supply placement, they own the final position.
- `touch::read_presses` returns the ids of your buttons tapped since your previous frame callback, oldest first. A tap completes when the finger lifts inside the button. It is empty outside `frame`, and a deferred frame keeps its taps for the next one.

Buttons are committed after a successful callback and removed on quarantine, unload or revocation.

## Asking the game to act

Separate grants let a mod request actions instead of only observing input. Each is deliberately narrow:

| Function | Grant | What it does |
| --- | --- | --- |
| `gameplay::pulse_jump` | `movement` | Stages one ordinary jump input for the next physics tick |
| `gameplay::set_movement_intents` | `movement` | Holds sprint or sneak as if bound; renew every focused gameplay frame, omission releases both |
| `gameplay::set_control_modes` | `movement` | Replaces toggle sprint/sneak and always-sprint while renewed every focused gameplay frame |
| `gameplay::pulse_attack` | `interaction` | One attack-press edge, only while the physical attack control is held |
| `item_use::pulse_use` | `item_use` | One item-use press edge for the next gameplay tick, only while the physical use control is held; skips only the client's use repeat delays |
| `gameplay::request_command` | `commands` | Sends a player command request after the frame returns; the command name must be granted. At most 4 per frame and 10 per second, each at most 128 bytes |
| `gameplay::set_attack_reach` | `interaction` | Sets local actor pick and attack range from 0 through `MAX_ENTITY_REACH_BLOCKS`; a value needs a current gameplay frame, and none restores game-mode rules |

The reach override can shorten or extend the local range, within the [current cap](../reference/limits.md). It does not enable attacks forbidden by the game mode, and block obstructions still apply.

Server movement, hunger and item-use restrictions still apply to all of these. The host stages local requests; it does not grant server permission to carry them out.

## Filtering attack and use actions

A filtered player mod additionally exports `filter_actions`. Where an ordinary mod sees edges, the filter decides them: the host passes an `action-frame` — an epoch, the physical and logical held states, and the `attack`/`item_use` transitions (`press`, `release`, `repeat`) — and the mod returns one accept/suppress decision per transition.

The earliest active exporting module is the sole owner, and filtering needs `controls`. It runs after ordinary frame callbacks, before gameplay, with independent fuel and no host-import calls. Suppressing a press blocks that gesture until physical release; suppressing a release retains the hold, and accepting the following press completes the outstanding release first. If the epoch changes — owner replacement, callback failure or an input-authority interruption — clear your policy state, because suppressed gestures stay suppressed until physical release. A trap or an invalid verdict quarantines the module and completes retained releases. Idle physical holds need no callback, so the host may skip idle calls.

In Rust, build against `mod_api::filtered_player_mod` instead of the ordinary bindings. See the [input reference](../reference/input.md) and the [gameplay reference](../reference/gameplay.md) for exact signatures.
