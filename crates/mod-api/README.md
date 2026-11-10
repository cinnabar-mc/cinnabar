# Personal mod SDK

Read the [modding book](../../docs/modding/src/introduction.md) for the public API.
The [world drawing helpers](../../docs/modding/src/appendix/drawing-helpers.md) and
[drawing materials](../../docs/modding/src/appendix/drawing-materials.md) document the Rust helpers.

World-draw 0.2 adds block grids and mesh clips. Components using 0.1 must rebuild.
See the [contract and migration notes](../../docs/modding/src/appendix/block-geometry.md).

Personal panel visibility is guest-owned. With the `controls` grant, read physical
keys from `input::read_controls`, decide whether your shortcut was pressed, then
call `panel::toggle()`. Use `panel::open()` or `panel::close()` when requesting
a specific state. The host rejects the removed `toggle_key` field and handles no
opening shortcut. The latest explicit opener owns the panel. See [packet panel imports](../../docs/modding/src/appendix/packet-panels.md)
for core reactors.
