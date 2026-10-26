# Introduction

Personal Cinnabar mods are WebAssembly components. A personal mod runs inside the client, in a sandbox with no filesystem, network or operating-system access of its own, and talks to the client through a capability API defined in WebAssembly Interface Types (WIT). The host only gives a mod the capabilities the player has granted it, and every call is bounded: a mod that tries to do too much in one callback is stopped rather than allowed to stall the game.

## What mods can do

Depending on the grants it holds, a mod can:

- read observations of the local player and the world around them: inventory, effects, position, nearby actors, combat and chat events, the server clock;
- look up item, food, potion and effect facts;
- react to local input, reserve keys, and show touch buttons;
- change local presentation: HUD labels and surfaces, nametag style and annotations, actor tints, the hurt overlay, tooltip lines, clickable chat actions, visual time and weather;
- move the camera, submit movement and item-use intents, and request granted commands;
- draw in the world with bounded primitives, retained geometry and sandboxed post-process passes, and read nearby terrain;
- store its own settings in a small companion file.

Personal mods have no direct world-editing API and get no raw packet access. Server guests can change their server's world; trusted packet mods can change session traffic. The latter's rules are covered in [Packet mods](guides/packet-mods.md).

## The kinds of mods

| Kind | What it is | Start with |
| --- | --- | --- |
| Personal client mod | One component you install on your own client. It runs in local and remote worlds under your grants; a server cannot control those grants. Targets the `extension` world, or the `player-mod` world when it needs screens beside container screens and the session's item and recipe catalogues. | [Getting started](guides/getting-started.md) |
| Paired client and server mod | One project with a server guest and a client part, shipped together in one `.cmod`. The server registers content such as blocks and items and exchanges typed messages with its client part, which the server offers to connecting players. | [Paired mods](guides/paired-mods.md) |
| Packet mod | One trusted component that relays every session packet: it sees each batch in both directions and may forward it unchanged, replace it or drop it, and can add resource packs to the session. | [Packet mods](guides/packet-mods.md) |

A fourth shape, the filtered player mod, is a personal mod that also exports an action filter deciding which attack and item-use edges reach the game. It is covered at the end of [Input](guides/input.md).

## How this book is organised

The guides take you from a first running mod to each area of the API:

- [Getting started](guides/getting-started.md) — tools, a new project, building, installing and reloading.
- [How mods run](guides/how-mods-run.md) — callbacks, the fuel budget, and what happens when a mod traps or overruns.
- [Grants](guides/grants.md) — how capabilities are requested, what the player sees, and asking for only what you need.
- [Settings and storage](guides/settings-and-storage.md) — persisting your mod's own data.
- [Input](guides/input.md), [Presentation](guides/presentation.md), [World and player](guides/world-and-player.md), [Camera and rendering](guides/camera-and-rendering.md) — the capability areas, one guide each.
- [Paired mods](guides/paired-mods.md) and [Packet mods](guides/packet-mods.md) — the two kinds that go beyond a personal mod.

Alongside the guides, the reference documents the WIT interfaces — for example [the presentation reference](reference/presentation.md) lists every function and type of the `presentation` interface with its exact signature. Read the guide for how an area fits together, and the reference when you need a signature or a field list.

This book does not yet have guides for HUD modules and menu panels. The `hud`, `panel` and `screen` interfaces are covered by the reference.
