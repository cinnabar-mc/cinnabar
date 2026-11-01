# Personal mod SDK

Read the [modding book](../../docs/modding/src/introduction.md) for the public API.
The [world drawing helpers](../../docs/modding/src/appendix/drawing-helpers.md) and
[drawing materials](../../docs/modding/src/appendix/drawing-materials.md) document the Rust helpers.

World-draw 0.2 adds block grids and mesh clips. Components using 0.1 must rebuild.
See the [contract and migration notes](../../docs/modding/src/appendix/block-geometry.md).

Personal panel visibility is guest-owned. With the `controls` grant, read physical
keys from `input::read_controls`, decide whether your shortcut was pressed, then
call `panel::set_open(desired_open)`. The host accepts old `toggle_key` JSON as
ignored metadata; F10 has no host fallback behavior. The latest explicit opener
owns the panel. See [packet panel imports](../../docs/modding/src/appendix/packet-panels.md)
for core reactors.
