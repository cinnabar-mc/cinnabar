# Presentation

The `presentation` interface restyles native presentation: how nametags read, what clings to actors, how items describe themselves, and which chat text is clickable. It needs the `hud` grant, and per-actor rows need more: a row for a player needs the `players` grant, a row for any other actor needs `entities`. Rows your grants do not admit never apply.

Every successful call is retained: it replaces your mod's previous value of that kind once the callback succeeds, and stays until you replace it, clear it, or the mod is unloaded or quarantined. A dimension transfer also clears these world-scoped policies. A callback may make at most 8 presentation calls; one more traps it.

```rust
use mod_api::bindings::cinnabar::extension::presentation::{
    self, Annotation, AnnotationPosition, TextSpan,
};
```

Types below are the generated Rust names for the WIT records in the [presentation reference](../reference/presentation.md).

## Nametag style

`set_nametag_style` takes an optional `nametag-style`; none releases your mod's style so the next mod's style, or native nametags, can apply. The style covers the whole tag: `hide` (and `hide_players`, `hide_passengers`), `show_self`, `show_when_hud_hidden`, `opacity` from 0 to 1, `scale` from 0.5 to 3, `border_width` from 0 to 8, `shadow`, and optional `rgba` colours for text, your own text, border and background. Present text colours replace the native ones; annotation spans keep their own colours.

## Annotations

`set_annotations` attaches your text spans to actors' nametags and, for players whose actor the client knows, to their player-list rows. Each `annotation` names a nonzero `runtime_id` (distinct within one call) and two span lists, `nametag` and `roster`, each with a position: `before` or `after` extend the name's line with exactly one host-inserted space between name and spans, while `above` and `below` put the spans on their own centred line. A player-list row has one line, so roster spans placed `above` or `below` follow the name as `after` does.

A span is plain single-line text with an `rgba` colour — no style codes. Spans draw end to end in list order with nothing inserted between them, so put separators in the spans themselves. Nametag spans on the local player also show on its own tag.

| Limit | Value | Constant |
| --- | --- | --- |
| Annotated actors per mod | 128 | `MAX_ANNOTATIONS` |
| Spans per surface | 4 | `MAX_ANNOTATION_SPANS` |
| Bytes per span | 32 | `MAX_ANNOTATION_SPAN_BYTES` |
| Tinted actors per mod | 64 | `MAX_ACTOR_TINTS` |
| Tooltip lines per mod | 256 | `MAX_TOOLTIP_LINES` |
| Mod lines on one item's tooltip | 8 | `MAX_ITEM_TOOLTIP_LINES` |
| Bytes per tooltip line | 96 | `MAX_TOOLTIP_LINE_BYTES` |
| Chat actions per mod | 32 | `MAX_CHAT_ACTIONS` |
| Bytes of chat text one action matches | 96 | `MAX_CHAT_ACTION_TEXT_BYTES` |
| Bytes in an action id | 48 | `MAX_CHAT_ACTION_ID_BYTES` |

All in `mod_api::presentation`. An empty span list leaves that surface native, so clearing one actor's annotation means republishing the set without it — every call replaces every annotation from your mod.

## Tints and the hurt overlay

`set_actor_tints` replaces your retained tints: a list of `runtime_id` and `rgba` colour pairs, drawn as an overlay while the actor is not hurt.

`set_hurt_overlay` sets the native damage overlay colours for actor bodies and their armour — `body` and `armor`, each optional, none keeping native. It is one policy: the earliest mod that sets either colour owns both.

## Tooltip lines

`set_tooltip_lines` replaces your mod's list of extra plain-text tooltip lines. Each line names an `identifier`, an optional `metadata` (none matches every variant), the `text`, and a colour. Use the `facts` interface from [World and player](world-and-player.md) to decide what a line should say: durability, food and potion facts are exactly the raw material tooltip lines are for.

## Chat actions

`set_chat_actions` makes spans of chat text clickable. Each action pairs a `text` to match inside a message of the open chat history with an `action` id of `[A-Za-z0-9_.-]`. A primary click or tap on the text closes the chat and reports your action id, with value 1, among the `input::read_controls` events on your next frame — reading it needs the `controls` grant, on top of the `hud` grant that sets the actions.

## When several mods present

The client merges loaded mods in load order:

- The earliest mod with a nonempty nametag style, a hurt overlay with either colour set, or a nonempty chat-action list owns that policy outright. Later mods' values for the same policy are ignored while the owner's stand; clearing it lets a later mod take over.
- For each runtime id and surface, the earliest mod whose grants admit the actor's kind owns the annotation and tint rows for that actor.
- Tooltip lines append in load order, up to the per-item cap.

So a mod loaded later cannot restyle what an earlier mod owns, but its tooltip lines still show. Design for coexistence: set only the policies your feature needs, because claiming the nametag style to change one colour also blocks every later mod's style.

## Sound events

Opt into `sound-filtered-player-mod` (or `audio-filtered-player-mod` when also using
action filtering) and request the `audio` grant to
receive `filter-sounds` callbacks. Each observation carries its exact routed
sound-definition name, optional world position and loop flag. Return one boolean
per event: true plays it, false cancels that request. Sound selection and conditions
belong in the component. Every admitted authorized filter sees the same batch;
any cancellation wins.

The callback runs before new sounds enter decoding or playback, once per audio
frame with at most 256 observations and 128 bytes per name. It cannot call host
imports and has bounded fuel, with a shared audio-frame pool. Observers run in load
order with their full callback allowance, or are skipped for that frame when the
pool cannot admit another full callback. Cache any other facts during ordinary
frame callbacks. Missing grants, absent callbacks, traps and malformed results
leave audio playing; failing components are quarantined. Excess or oversized
observations pass through. Unloading immediately removes the observer.

Cancellation discards a one-shot rather than delaying it. Previously approved
decodes are not offered again, and playing sounds are not stopped. Managed loops
offer a new start when they need one, so a later callback may admit a previously
cancelled loop. Resource packs are observed after routing to their sound definitions.
