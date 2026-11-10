# Presentation

The `presentation` interface restyles native presentation: how nametags read, what clings to actors, how items describe themselves, and which chat text is clickable. It needs the `hud` grant, and per-actor rows need more: a row for a player needs the `players` grant, a row for any other actor needs `entities`. Policies for every actor, such as a kind's default nametag visibility or an every-actor tint, need only `hud`. Rows your grants do not admit never apply.

Every successful call is retained: it replaces your mod's previous value of that kind once the callback succeeds, and stays until you replace it, clear it, or the mod is unloaded or quarantined. A dimension transfer also clears these world-scoped policies. A callback may make at most 8 presentation calls; one more traps it.

```rust
use mod_api::bindings::cinnabar::extension::presentation::{
    self, Annotation, AnnotationPosition, TextSpan,
};
```

Types below are the generated Rust names for the WIT records in the [presentation reference](../reference/presentation.md).

## Nametag style

`set_nametag_style` takes an optional `nametag-style`; none releases your mod's style so the next mod's style, or native nametags, can apply. The style covers how every tag looks: `opacity` from 0 to 1, `scale` from 0.5 to 3, `border_width` from 0 to 8, `shadow`, and optional `rgba` colours for text, your own text, border and background. Present text colours replace the native ones; annotation spans keep their own colours.

## Nametag visibility

`set_nametag_visibility` decides which tags draw. It takes a default `nametag-visibility` for players' tags, the local player's own included, another for every other actor's tags, and a list of per-actor overrides by runtime id that replace their kind's default. Each visibility is one of:

- `native`: as the client draws it. The local player's own tag shows only when the player's settings or an annotation show it, and no tag draws while the player hides the HUD.
- `shown`: also the local player's own tag outside first person, and the tag while the HUD is hidden and no menu is open.
- `hidden`: never drawn.

No visibility draws a tag the actor lacks or the player's own name settings remove. The defaults need only `hud`; overrides follow the per-actor grant rules. Build features from these and the observations: hide every player's tag with a `hidden` players default; show your own tag with a `shown` players default; hide passengers' tags by reading `player_state::read_actors` and overriding each actor whose `vehicle` is set, and your own through `read_world`'s `vehicle`; show tags with the HUD hidden by switching to `shown` while `hud::read_hud_hidden` reports it. Overrides can only name actors you observe — the nearest 16 within 128 blocks at a time — so an override reaches farther actors only while you retain it.

## Annotations

`set_annotations` attaches your text spans to actors' nametags and, for players whose actor the client knows, to their player-list rows. Each `annotation` names a nonzero `runtime_id` (distinct within one call) and two span lists, `nametag` and `roster`, each with a position: `before` or `after` extend the name's line with exactly one host-inserted space between name and spans, while `above` and `below` put the spans on their own centred line. A player-list row has one line, so roster spans placed `above` or `below` follow the name as `after` does.

A span is plain single-line text with an `rgba` colour — no style codes. Spans draw end to end in list order with nothing inserted between them, so put separators in the spans themselves. Nametag spans on the local player also show on its own tag.

| Limit | Value | Constant |
| --- | --- | --- |
| Annotated actors per mod | 128 | `MAX_ANNOTATIONS` |
| Spans per surface | 4 | `MAX_ANNOTATION_SPANS` |
| Bytes per span | 32 | `MAX_ANNOTATION_SPAN_BYTES` |
| Actors with a nametag visibility override per mod | 128 | `MAX_NAMETAG_OVERRIDES` |
| Actors with their own tints per mod | 64 | `MAX_ACTOR_TINTS` |
| Tooltip lines per mod | 256 | `MAX_TOOLTIP_LINES` |
| Mod lines on one item's tooltip | 8 | `MAX_ITEM_TOOLTIP_LINES` |
| Bytes per tooltip line | 96 | `MAX_TOOLTIP_LINE_BYTES` |
| Chat actions per mod | 32 | `MAX_CHAT_ACTIONS` |
| Bytes of chat text one action matches | 96 | `MAX_CHAT_ACTION_TEXT_BYTES` |
| Bytes in an action id | 48 | `MAX_CHAT_ACTION_ID_BYTES` |

All in `mod_api::presentation`. An empty span list leaves that surface native, so clearing one actor's annotation means republishing the set without it — every call replaces every annotation from your mod.

## Tints

`set_actor_tints` replaces your retained tints. Each `actor-tint` names a `target` — `every-actor`, or one `actor` by runtime id — a `state`, `hurt` or `not-hurt`, and optional `body` and `armor` colours, at least one set; an absent colour keeps that part native. The `hurt` state lasts exactly as long as the native damage flash, whose timing the client keeps. While hurt, the body colour replaces the native damage overlay only where that overlay draws, so items the flash skips stay untinted. In each state an actor's own tint replaces the every-actor tint, both colours together. Recolouring the damage flash for every actor is one `every-actor` `hurt` tint; a per-actor overlay is a `not-hurt` tint for that actor with the same body and armour colour.

## Tooltip lines

`set_tooltip_lines` replaces your mod's list of extra plain-text tooltip lines. Each line names an `identifier`, an optional `metadata` (none matches every variant), the `text`, and a colour. Use the `facts` interface from [World and player](world-and-player.md) to decide what a line should say: durability, food and potion facts are exactly the raw material tooltip lines are for.

## Chat actions

`set_chat_actions` makes spans of chat text clickable. Each action pairs a `text` to match inside a message of the open chat history with an `action` id of `[A-Za-z0-9_.-]`. A primary click or tap on the text closes the chat and reports your action id, with value 1, among the `input::read_controls` events on your next frame — reading it needs the `controls` grant, on top of the `hud` grant that sets the actions.

## When several mods present

The client merges loaded mods in load order:

- The earliest mod with a nonempty nametag style or a nonempty chat-action list owns that policy outright. For each actor kind, the earliest mod whose default visibility is not `native` owns that default; for each tint state, the earliest mod with an every-actor tint owns it. Later mods' values for the same policy are ignored while the owner's stand; clearing it lets a later mod take over.
- For each runtime id and surface, the earliest mod whose grants admit the actor's kind owns the annotation, visibility override and tint rows for that actor; tint rows are owned per state.
- Tooltip lines append in load order, up to the per-item cap.

So a mod loaded later cannot restyle what an earlier mod owns, but its tooltip lines still show. Design for coexistence: set only the policies your feature needs, because claiming the nametag style to change one colour also blocks every later mod's style.

## Event handlers

Some host events can be changed or cancelled by an optional `handle-<event>` export.
Each handler receives a context with the event's facts and its changeable values and
returns it. The host applies only the changeable values. Include the handler worlds
you implement (`sound-handler`, `teleport-handler`) in your component's world, or use
`handled-player-mod`, which also includes the action filter.

Handlers run in load order, each receiving the previous handler's changes, and
cancellation is sticky. They cannot call host imports, so cache any other facts during
ordinary frame callbacks. Each call has `HANDLER_CALL_FUEL` within a shared allowance
per event family; handlers that the remaining allowance cannot cover are skipped.
Missing grants and absent handlers leave the event unchanged. Traps and invalid values
quarantine the component and keep the context it received.

### Sounds

With the `audio` grant, `handle-sound` receives each new playback request before it
enters decoding or playback: its exact routed sound-definition name, optional world
position and loop flag. Set `cancelled` to discard it. At most 256 requests per audio
frame and 128 bytes per name are offered; excess or oversized requests play unhandled.

Cancellation discards a one-shot rather than delaying it. Previously approved
decodes are not offered again, and playing sounds are not stopped. Managed loops
offer a new start when they need one, so a later call may admit a previously
cancelled loop. Resource packs are observed after routing to their sound definitions.

### Teleports

With the `camera` grant, `handle-teleport` receives each committed local-player
MovePlayer with a finite rotation, before the rotation applies: the mode, the server's
rotation, the view's rotation just before the move, and the rotation about to apply.
Change `rotation` to replace it, for example to `presented` to keep the current view.
Position, physics and the teleport acknowledgment always follow the server.
