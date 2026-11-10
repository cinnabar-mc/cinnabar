# Spawn egg actor icons

Spawn eggs select their sprites from the chosen client entity's `spawn_egg`
declaration, including its texture alias and index. Named villager and zombie
villager eggs use the current `_v2` actors when present. Their current artwork
remains authoritative, including in the pinned sample. Authored definitions
retain their selection.

The generic `minecraft:spawn_egg` is a separate compatibility path. Its auxiliary
names an actor type, rather than a position in the `spawn_egg` atlas array.
Villager selectors 15/115 and zombie-villager selectors 44/116 use the selected
current actor's icon when that replacement exists. The original actor remains
the fallback when its replacement is absent. In particular, auxiliary 15 must
not select atlas entry 15, which is a squid icon in the pinned pack. The legacy
server stack therefore draws the same current artwork as the named egg.

NPC selector 51 follows its actor's declaration too. Although the retained item
registry omits `minecraft:npc_spawn_egg`, servers can announce that named item.
Compile the selected NPC actor icon for both that identifier at metadata zero
and the legacy generic selector. Actor admission still requires either a
registered named egg or a reviewed legacy selector; unrelated custom actors do
not gain invented item routes. The generated actor-selector facts cover every
spawn-egg actor in the pinned protocol.

Regressions cover current villager artwork even when a legacy definition is
present, actor version selection, authored aliases and indices, and server-named
NPC eggs through runtime icon rasterization. Optional installed-pack fixtures
are read in place without copying resources. They check current named textures,
replacement actor provenance, both NPC forms and all canonical retail egg
icons. A target/sample-pin guard requires reviewing the selector facts on updates.

The 91 generic selectors are generated from the pinned protocol's actor
identifier list and the existing compatibility-actor rules, rather than kept as
a hand-written subset. The source is the
[pinned BedrockData actor list](https://github.com/pmmp/BedrockData/blob/7e91146842837b2267a8e4d8bf8b8dab5e482970/entity_identifiers.nbt).
To verify the generated facts against an existing local copy, run:

```sh
python3 tools/fixturegen/generate_spawn_egg_selectors.py /path/to/entity_identifiers.nbt --check
```

The generator verifies the pinned Git blob and emits only identifier/number
facts. It never copies the input resource.
The installed-pack regression compares all 91 selectors with their named actor
icons, in addition to checking every canonical retail egg.

Gameplay comparisons confirm the modern villager artwork for a legacy trader
egg and the visible server-announced NPC appearance item. To reproduce, inspect
those items in a server shop and hotbar after `make play`. The installed-pack
regression covers the remaining selector/icon combinations.
