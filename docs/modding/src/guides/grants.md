# Grants

Every capability beyond the ungated basics is off until the player grants it. A personal mod with no grants can still run `init` and `frame`, set a HUD label, read the demo action, exchange cues and read its own remaining fuel. Gated imports check their grants, but denial is not always an error: some reads return none or an empty list. Others return an error such as `"clock capability denied"`. A personal mod can load with fewer grants than it requested and handle those denials.

## How a grant is given

The flow is the player's, not the mod's:

1. When you import a component in Settings → Mods, the client shows the permissions the mod requests. The player reviews them and enables the mod.
2. A component may declare what it wants in a top-level `cinnabar:grants` custom section containing a `ModGrants` JSON object. The declaration requests; it grants nothing by itself. A personal component with no declaration has no declared requests.
3. The player can revoke individual grants, or all of them, at any time in Settings. Permission changes reload that mod, so it restarts under the new grants.

Some capabilities are marked sensitive in the permission list because they can change combat, movement or gameplay input: `camera`, `item_use`, `interaction`, `packet_delay`, `packets`, `controls`, `movement`, `commands`, `keys` and `inventory`. Expect players to read those twice.

Server consent does not control personal-mod grants. A personal mod runs under its player's grants in local and remote worlds alike. Paired mods are different: the server offers the client part, and its permissions are declared in the project — see [Paired mods](paired-mods.md).

### Declaring the request

The section payload is UTF-8 JSON, for example `{"environment":true,"settings":true}`; omitted fields default to denied. It must be at most 16 KiB, use known `ModGrants` fields, and occur only once at the top level of the final component. A section nested in the core module is not a component declaration.

There is currently no author-facing embedding command in `mod-host pack` or `cinnabar-cxb`, and the personal examples do not embed this section. You must add it to the finished component with your own WebAssembly section tooling; `pack` alone does not create it. The six retained-drawing grants (`world_draw`, `entity_observe`, `entity_highlight`, `render_through_walls`, `render_material`, `render_post`) require both the component's explicit request and the player's grant. Directory packages declare their requests in `mod.toml` instead. Packet modules without a section have an inferred request for `packets` and their detected native capabilities, including WASI when needed.

## The capabilities

Grant names below are the `ModGrants` field names; the label is what the permission controls show.

| Grant | Label | Admits |
| --- | --- | --- |
| `player_state` | Player status | `player-state` reads other than the actor and actor-life reads; also the `facts` interface |
| `clock` | Clock | `clock.read-local-time` |
| `environment` | Environment | Visual time and weather overrides, local menu branding |
| `fullbright` | Fullbright | `environment.set-fullbright`, separately from `environment` |
| `players` | Player poses | Remote player rows in actor reads and gameplay frames; per-actor presentation rows for players |
| `entities` | Entities | Non-player actor rows and `gameplay.read-mobs` |
| `hud` | HUD | HUD surfaces, nametag and annotation policy, hurt overlay, tooltip lines, chat actions |
| `controls` | Controls | Key edges, reserved keys, text input, touch buttons, the panel, chat-action clicks |
| `camera` | Camera rotation | Camera rotation, rigs, view tilt and scale, teleport aim preservation |
| `interaction` | Attack reach | Bounded attack reach and held-attack press pulses |
| `item_use` | Item use | Item-use press pulses |
| `movement` | Movement | Movement snapshots, jump pulses, movement intents and control modes |
| `commands` | Commands | Requesting the named commands only; the list is part of the grant |
| `packet_delay` | Packet delay | Bounded post-login packet delay and the relayed-position read |
| `settings` | Settings file | The companion settings file in [Settings and storage](settings-and-storage.md) |
| `render` | Rendering | Post-process passes and world primitives |
| `render_depth` | Scene depth | Lets render passes read scene depth |
| `world_draw` | World drawing | Retained world geometry |
| `entity_observe` | Entity observation | Session-authorized actor observations for drawing |
| `entity_highlight` | Entity highlights | Drawing attached to authorized actors |
| `render_through_walls` | Through walls | Elevated drawing and protected world observations |
| `render_material` | Drawing materials | Procedural drawing materials |
| `render_post` | Post effects | Scene post effects over retained drawing and masks |
| `terrain` | Terrain | `terrain.read-region` and `terrain.find-blocks` |
| `screen` | Screens | A `player-mod` package's overlay and view beside container screens |
| `items` | Items | Reading the session's items (`player-mod` packages) |
| `recipes` | Recipes | Reading the session's recipes (`player-mod` packages) |
| `keys` | Keys | Delivering a package's declared keys |
| `inventory` | Inventory requests | Creative takes and recipe fills the vanilla screens can make |
| `block_highlights` | Block highlights | A packet mod's full-block highlights in its render frame |
| `packets` | Session packets | A packet mod relaying every session packet |
| `wasi` | WASI runtime | WASI clocks, randomness and captured output for a packet mod; never files or network |

The [facts](../reference/facts.md), [player-state](../reference/player-state.md) and other reference pages name the grant each function checks.

## Requesting only what you need

Ask for the smallest set that implements your feature, for three practical reasons. The player sees the list before enabling your mod, and a short list is easier to accept. Sensitive grants get extra scrutiny. And every grant is a promise your code has to keep: with `commands`, for example, the grant covers only the command names it lists (`MAX_COMMAND_GRANTS` is 8 names, each at most `MAX_COMMAND_BYTES` bytes), so declare the exact names you send.

Handle denial as a normal case. The time-changer example shows the pattern: it attempts the override, and on `Err` it publishes a "denied" label and keeps its previous state instead of trapping.

```rust
use mod_api::bindings::cinnabar::extension::environment;

let ticks = Some(6_000);
match environment::set_time_override(ticks) {
    Ok(()) => { /* update the label, keep the new mode */ }
    Err(_) => { /* tell the player, keep the old mode */ }
}
```

From `examples/mods/time-changer/src/lib.rs`, shortened. A denied import is information, not a failure of your mod.
