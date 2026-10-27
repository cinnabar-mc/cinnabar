# Managed local terrain observations

Personal mods may import `cinnabar:extension/local-terrain@0.1.0` to observe bounded
regions in another dimension of the currently connected, automatically managed
native BDS world. This is separate from `terrain`, which observes client-loaded
terrain near the player. It does not select a world directory or remote server.

`local_terrain` consent allows the launcher to install a fixed, world-local Script
API companion when opening a native BDS world. Existing behavior-pack references
are preserved. Grant permission before opening the world; an already running world
must be reopened to activate the companion. Worlds opened without consent do not
receive it. Existing companion registrations remain when permission is later revoked,
but observations and their temporary loading demands stop. No cheats or experiments
are enabled automatically.

`read-region(query, after)` takes a native dimension number, minimum block position,
three extents and a `load` flag. Bounds and aggregate quotas come from
`mod_api::local_terrain`: at most four retained queries, each axis at most 16 blocks.
Cells use x,z,y order. Each known cell indexes a palette of canonical named states;
65535 means unavailable, never air. A result is `pending`, `unchanged(stamp)` or
`changed(region)`. Stamps identify a managed server opening and content revision.
Do not retain them across connections.

Setting `load` additionally requires `local_terrain_load`. It allows a bounded
temporary ticking area around the query, which affects simulation there. Leave it
false for already-loaded observations only. Abandoned queries expire, and removing
the last reader cancels its aggregate demand. Closing the world stops the observer
and requests temporary-area cleanup before the server saves. A crash can prevent
normal teardown; these are Script API temporary areas, not persistent command areas.

Requests are sampled off the client frame thread under a shared cell budget. A scan
may span server ticks and is not an atomic transaction. The companion reports only
the block permutation returned by BDS: solved lighting, biome information, secondary
layers, entities and block-entity contents are not supplied. Named states are converted
from Script API values to native typed canonical states; registry IDs are not shared.

This initial adapter supports native BDS only. Missing Script API access, companion
activation failure, container/translated BDS, Dragonfly and remote servers report
unavailable. Responses are bounded and fenced to their process nonce, request ID and
sequence. The client also checks the exact `local:<world-id>` destination before
providing the source. The interface exposes no arbitrary console commands.

The engine does not determine portal links, choose destination coordinates or place
preview geometry. Those policies belong to the mod. In particular, observing a prior
crossing does not establish which exit vanilla will select after portals change.
