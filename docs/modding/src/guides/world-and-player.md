# World and player

These interfaces tell a mod what the client already knows: the `player-state` observations, the `facts` tables, the `clock`, the local `environment` overrides, and `events`, the cue channel between mods. Observations are read-only and session-bound. Anything the client does not know is reported as absent — an `option` left none — never estimated.

## Player state

`player-state` needs the `player_state` grant. The reads fall into families; each returns records carrying a `session` and `dimension`, so you can tell a stale reading from a current one. Compare session-scoped values only within one session.

| Read | Returns |
| --- | --- |
| `read_snapshot` | The presented inventory (exactly 36 slots, hotbar first), armour (4 slots), offhand, selected slot, and visible effects sorted by effect id |
| `read_revision`, `read_revisions`, `read_equipment`, `read_effects` | Exact content tokens, and the equipment/effects halves separately, so effect ticks do not invalidate equipment |
| `read_world` | Session-fenced feet position, alive state, the stable world identity and the local player's runtime id |
| `read_motion` | Yaw and pitch (YXZ degrees, yaw left, pitch up), velocity in blocks per tick, on-ground |
| `read_vitals` | Hunger, maximum hunger and saturation as attribute points, and the held item's server food definition |
| `read_actors`, `read_actors_limited` | At most 16 nearby actors within 128 blocks; the limited form reads a prefix of 1 to `MAX_OBSERVED_ACTORS` and adds each actor's maximum health |
| `read_actor_lives` | Per runtime id, the actor's current `life` and whether it is dead; at most `MAX_ACTOR_LIFE_READS` (64) ids |
| `read_combat`, `read_combat_lives` | Retained combat events — `attack-admitted`, `hurt`, `death`, `totem` — at most 4 per read, and the actor life at each event |
| `read_chat` | Newest 32 retained received messages, filtered by sequence, at most 4 per read, each text at most 256 bytes |
| `read_world_time` | The server world clock: absolute ticks last set, advanced 20 per second while the daylight cycle runs, 24,000 per day |
| `read_sidebar` | The resolved sidebar title and ordered rows, when the server has one |
| `read_ping` | Measured upstream round-trip milliseconds |
| `read_fps` | Measured completed client frames per real second, never the configured limit |

Players among actors need the `players` grant, other actors need `entities`; rows you cannot see are filtered out before you count anything. `read_snapshot` and its revision family are available while a screen owns input, because they carry no camera, interaction or packet authority. Several reads share one callback read allowance, so prefer the narrow read: check `read_revisions` and only re-read equipment when its token moved, instead of pulling a full snapshot every frame.

Two cautions belong in your head while reading combat events. `attack-admitted` is outbound intent, not server confirmation — it records that the client admitted your attack. And `hurt` carries no attacker attribution. A life value changes whenever an actor spawns again or dies; death events carry the life after the death, which is how you tell one life of a runtime id from the next.

## Facts

The `facts` interface answers questions about items and effects from the pinned tables and the installation's food definitions, without a session. It rides on the `player_state` grant but has its own callback allowance: `MAX_FACT_READS` (128) reads per callback, counted apart from player-state reads, and one more traps the callback.

- `read_item(identifier, metadata)` returns the pinned maximum durability, the food definition and the carried potion effects for an item — none when neither table knows the identifier. Tipped arrows report the carried potion's effects; lingering potions report a quarter of the potion's durations. Server item components are not consulted; `read_vitals`' `held_food` reports those.
- `read_foods` returns every item the installation's food table defines, sorted by identifier, at most `MAX_FOOD_ITEMS` entries. It counts as one read, which makes it the cheap way to build a food index in `init`.
- `read_effect(effect_id)` returns an effect's localization key and HUD icon texture name, none for an unknown id.

## Clock

`clock::read_local_time` returns `unix_ms` and the local zone's current `utc_offset_seconds`, under the `clock` grant. It is wall-clock time for timestamps and daily resets. For anything the server can observe or that must advance with the game, use `read_world_time` instead — visual time overrides never apply to it.

## Environment: local visual overrides

The `environment` interface changes local presentation only, never server state:

- `set_time_override(ticks)` fixes visual time to 0–23999 ticks; none restores the tracked server clock. Needs `environment`.
- `set_weather_override(clear | rain | thunder)` sets the local visual weather target; none restores tracked server weather. Needs `environment`.
- `set_fullbright(enabled)` retains a local lighting override and needs the separate `fullbright` grant. Server light data is unchanged.

`examples/mods/time-changer` is a complete tour of the time override.

## Events between mods

The `events` interface is a cue channel shared by loaded mods; it carries no authority and needs no grant. `events::emit(name, values)` publishes a named cue with up to `MAX_CUE_VALUES` (8) floats after your frame returns, to the host and every loaded mod. `events::poll` returns the cues other mods committed last frame — the same bounded list for every call in a callback, at most `MAX_INCOMING_CUES` (64).

| Limit | Value | Constant |
| --- | --- | --- |
| Cues emitted per frame | 16 | `MAX_CUES_PER_FRAME` |
| Bytes in a cue name | 32 | `MAX_CUE_NAME_BYTES` |
| Values per cue | 8 | `MAX_CUE_VALUES` |
| Incoming cues per callback | 64 | `MAX_INCOMING_CUES` |

Cues are how co-operating mods coordinate — one mod detects, another displays — without either holding a grant over the other. Names are a shared namespace, so prefix them with your mod's name.

See the [player-state](../reference/player-state.md), [facts](../reference/facts.md), [clock](../reference/clock.md), [environment](../reference/environment.md) and [events](../reference/events.md) reference pages for exact record fields.
