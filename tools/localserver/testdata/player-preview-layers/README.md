# Player preview layers

Original artwork and geometry for checking HUD player-controller rendering. The textures in
`tools/localserver/testdata/player-preview-layers/textures/` are original fixture artwork.
The panel and
marker are ordinary images; the striped rectangle is a second player render-controller layer.
A static bone animation moves that rectangle away from its renderer's nominal control bounds.
Its UV offset depends only on camera yaw, with two horizontal texture repetitions.

Copy this directory beneath a local server world's `resources` directory before starting
`bedrock-local-server -dir WORLD -addr 127.0.0.1:19221 -game-mode creative -time noon`.
Use the same viewport, GUI scale and camera yaw for both clients. No elapsed-time animation
phase needs to be synchronized. The fixture does not require an account or installed mods.

The focused client UI regression uses the same entity, geometry, animation and texture inputs,
then checks rasterized pixels with an unclipped ancestor and a clipping ancestor. The menu
preview retains its separate context; this fixture does not establish general menu material
or render-controller parity.
