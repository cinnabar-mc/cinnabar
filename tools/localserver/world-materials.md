# World material showcase

The `materialfx` Experience shows what programmable world materials can do inside the host's
rules. Its client part (`examples/experiences/material-fx/client`) declares one guest `shade`
material per effect in `effects::EFFECTS` and binds them to the Experience's own entities and
blocks; its server half (`examples/experiences/material-fx/server`) registers the blocks; this
local server fixture (`-material-fx`) offers the entities and adds `/materialfx`. All art is
original and generated in code: nothing under `textures/` is committed.

World materials are developer-only: the client runs them only with
`CINNABAR_DEV_SERVER_EXPERIENCES=1`, and production clients refuse the permission.

## Run it

From the repository root:

1. Build the local server, the Experience runtime and `cinnabar-cxb`:

   ```sh
   make local-server
   ```

2. Build the showcase and seed it into the client's bundle cache:

   ```sh
   cargo run -p material-fx -- --out .local/material-fx
   ```

   A client built from this checkout (`make play`) keeps its user data in the repository's
   `.local`, which the tool seeds by default. For an installed client, pass its user data
   directory, for example `--user-data <directory>`. The server's offer
   names `https://cxb.invalid`, a host that never resolves, so a client whose cache was not
   seeded stops the Experience with a message saying so.

   This builds both guests for `wasm32-unknown-unknown`, writes the server artifact under
   `.local/material-fx/experiences/materialfx`, keys under `.local/material-fx/keys` (kept across
   runs, so a client that pinned the server keeps trusting it), the signed client part under
   `.local/material-fx/cxb`, and prints the server command line.

3. Build the server and runtime with `make local-server` if needed, then start the server
   with the printed command (it prints absolute binary paths), for example:

   ```sh
   target/release/bedrock-local-server -dir .local/material-fx/world -addr 127.0.0.1:19132 \
     -material-fx -experiences .local/material-fx/experiences \
     -experience-runtime target/release/experience-runtime \
     -extension-key .local/material-fx/keys/server.seed \
     -extension-audience 127.0.0.1:19132 \
     -extension-cxb .local/material-fx/cxb
   ```

4. Start the client with Experiences enabled and join `127.0.0.1:19132`, accepting the
   Experience's consent prompt:

   ```sh
   CINNABAR_DEV_SERVER_EXPERIENCES=1 make play
   ```

   With the switch, `make play` also builds the media helper with its AV1 decoder, which needs
   `dav1d` and `pkg-config`.

5. Run `/time set night` (or stand in a cave) so emission stands out, then `/materialfx`.

## Commands

| Command | Effect |
| --- | --- |
| `/materialfx` or `/materialfx spawn` | Builds the gallery in front of you (your facing snaps to the nearest compass direction), clearing the one you built before, sends the client part its layout and binds every material over about a second |
| `/materialfx revoke` | Unbinds every material: the mobs draw as their plain skins and the blocks as their Bedrock textures again, with no reconnect |
| `/materialfx restore` | Binds them again from the materials the client part kept |
| `/materialfx boom` | A shockwave ring races out across the arena floor from its centre |
| hitting the liquid metal mob | Ripples its chrome from where you struck it and sends a shockwave across the arena from its feet |
| `/materialfx heartbeat <0-100>` / `heartbeat off` | The arena floor beats red in double pulses, faster as the boss health you give falls |
| `/materialfx thermal` | Toggles thermal vision on every mob |
| `/materialfx corrupt` / `cleanse` | Purple corruption creeps block by block across the stone wall from its right end, or the wall is clean again |
| `/materialfx cut <0-100>` / `cut sweep` | Holds the wraith's dissolve at that percentage, or returns it to its four-second sweep |

Every server-driven change is a parameter update that compiles nothing. Shockwaves, hits and the
corruption start at a moment on the session clock, which follows the wall clock modulo 4096 s,
so the server times them and every client agrees. The screen gets every player's position twice
a second and the portal your distance whenever it changes by a quarter block.

## Layout

From where you stand, facing forward (right is your right hand):

- 4 blocks ahead, left to right: the liquid metal mob, the lenticular mob, the void walker, the
  hologram and the wraith;
- 6 to 12 blocks ahead, in the ground: the 9×7 arena floor, with a 3×3×3 black hole floating 3
  to 5 blocks above its middle;
- 7 blocks ahead: the ship in a bottle 6 to the right and the lava lamp 6 to the left; 9 ahead the
  snow globe on the right; 11 ahead 2×2 clusters of breathing diamond ore (right) and redstone
  ore (left);
- 15 blocks ahead, a stone wall 17 wide and 5 high: from the right, the hollow aquarium and
  mineshaft with a carved rune above each, the 2×2 living painting, the 2×3 star portal and the
  4×3 screen.

## What each effect should look like

Everything except the black hole snaps to the 16-texel grid of its block or the 64-texel grid of
its skin and uses a small palette, so even the traced 3D reads as Minecraft pixels.

- **Ship in a bottle** (premultiplied). A glass cube with a one-texel frame and a glint, holding
  a floating island built of texel voxels: grass over dirt over a stone point, a cottage with a
  lit window, a hut, a tree, a cloud, and a stream that spills off the edge as a waterfall into a
  pool, its water rippling down. A pixel sun circles above and lights the voxel faces in three
  steps. Each pixel traces its ray through the 16³ grid, so the world sits inside the glass at
  its true depth from every side; around it you see the room through the glass.
- **Snow globe** (premultiplied). A wooden-framed glass block with a snowy mound, a pine with a
  gold star and a cabin with a lit window; one-texel flakes fall on three depth layers and drift.
- **Lava lamp** (opaque). Glowing orange blobs built of texel voxels rise, sink and merge in
  violet liquid between metal caps.
- **Hollow aquarium** (opaque). The block opens onto a tank three blocks deep: sand with rippling
  light, swaying kelp, rising bubbles and pixel fish swimming at three depths, smaller the deeper
  they are, the water darkening with depth.
- **Hollow mineshaft** (opaque). A tunnel ten blocks deep with timber frames and rails running
  into the dark towards a flickering torch that lights it.
- **Arena floor** (opaque). Dark riveted plates. `boom` and hits send a white-cored cyan ring
  with sparks out at nine blocks a second that fades in two and a half seconds; the heartbeat
  pulses red rings and veins.
- **Black hole** (premultiplied, smooth). A black shadow ringed by a thin photon ring; the
  accretion disk wraps over the top and under the bottom where its far side is lensed, hotter
  towards the centre and brighter and bluer on the side spinning towards you; the starry sky
  behind is bent around it. Beyond its lensing radius the cluster is clear.
- **Living painting** (opaque). A gilded 32-pixel night scene: an owl on a branch under the
  moon, drifting clouds, twinkling stars and blinking fireflies. Its pupils move towards wherever
  you stand and it blinks.
- **Breathing ore** (opaque). Diamond specks glint as you move your head; redstone veins run
  across neighbouring blocks with pulses of light travelling along them.
- **Corruption** (opaque, the wall's stone). After `corrupt`, a purple infection with a glowing
  edge eats across each block from the side facing its source, block after block, leaving
  pulsing violet veins.
- **Screen** (opaque). A green phosphor radar with range rings and a turning sweep; you are a
  cross and other players blinking blips, moving live.
- **Carved rune** (opaque). Runes cut into dressed stone; their walls shift and hide part of the
  glowing groove floors as you move.
- **Star portal** (opaque). An obsidian frame opening onto a vortex: violet bands spiral into a
  blazing eye, stars sit deep in the walls and pixel sparks stream inward. It turns faster and
  burns brighter as you come closer.
- **Wraith** (cutout). The pale robed figure comes apart texel by texel in a noise pattern with a
  two-step burning edge and flickering embers, and reassembles every four seconds.
- **Hologram** (additive). A cyan projection with a glowing rim, one-texel scanlines drifting up,
  flicker and the odd glitch slipping a band sideways.
- **Void walker** (cutout). Its body is a window into space: stars and nebula stay fixed in the
  world as it moves, and a violet rim of pixel stars sparkles at its silhouette.
- **Lenticular mob** (cutout). Shows a gold mask, a creeping green face or a skull depending on
  the angle you see each surface from, shimmering at the switch.
- **Liquid metal** (cutout). Pixel-crisp chrome reflecting a sunset skybox; it ripples when hit.
- **Thermal vision** (`thermal`, cutout). Every mob in a six-step heat palette, hottest where it
  faces you.

## Tests

- `cargo test -p material-fx`: the showcase builds an artifact the Experience runtime loads, with
  exactly the blocks the tool draws a Bedrock look for, and a client part bundle the client
  verifies.
- `cargo test -p cinnabar --lib material_fx`: the real client part, as the tool builds it, runs in
  the real developer helper; it declares all twenty materials over several callbacks, the client
  validates and publishes them within their budgets, a shockwave updates parameters without a
  new shader, thermal vision rebinds the mobs and back, revoke restores every Bedrock material and
  restore binds them again.
- `cargo test -p render --test it material_fx`: every shader passes the sandbox's per-pixel
  bound; the traced pieces are checked through camera rays from several sides (the bottle's voxel
  at its place, the globe's tree and falling snow, the lamp's moving blobs, the black hole's
  shadow, ring and reach, the hollow rooms, the rune's occluding walls, the portal's reaction to
  distance) and a wall of bottles traces each copy on its own; the surfaces and mobs show their
  distinguishing pixels (the ring where it has travelled, the heartbeat, glints and pulses, the
  spreading corruption, the owl's pupils, radar blips, world-fixed stars, the lenticular faces,
  the chrome's ripple, the heat palette, the burning edge and the hologram's rim).
- `cargo test -p render --lib scale_tests` and `many_bound_actors`: published materials request
  their pipelines before any bound block is in view, and 1 024 bound sections or 1 000 bound
  actors request no further pipelines.
- `go test -run 'MaterialFx|Showcases' .` in `tools/localserver`: the pack declares each mob, the
  commands become control records of the declared channel, and the gallery holds every piece in
  front of the player at foot level for each facing.
