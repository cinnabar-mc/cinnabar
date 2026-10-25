# World material showcase

The `materialfx` Experience shows what programmable world materials can do inside the host's
rules. Its client part (`examples/experiences/material-fx/client`) declares five guest `shade`
materials and binds them to the Experience's own entities and blocks; its server half
(`examples/experiences/material-fx/server`) registers the blocks; this local server fixture
(`-material-fx`) offers the entities and adds `/materialfx`. All art is original and generated in
code: nothing under `textures/` is committed.

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
| `/materialfx` or `/materialfx spawn` | Spawns the three entities in a row four blocks ahead, places a 3×2 energy wall on the left and a 2×3 star portal on the right six blocks ahead, and binds every material |
| `/materialfx revoke` | Unbinds every material: the entities draw as their plain skins and the blocks as their Bedrock textures again, with no reconnect |
| `/materialfx restore` | Binds them again from the materials the client part kept |
| `/materialfx flash` | A damage flash on the sentinel: white at once, fading out over about half a second, as six parameter updates that compile nothing |
| `/materialfx status <none\|poison\|burning\|frozen>` | Tints the sentinel with a pulsing status colour, a parameter update |
| `/materialfx cut <0-100>` / `/materialfx cut sweep` | Holds the wraith's dissolve at that percentage, or returns it to its four-second sweep |

`init` can validate four materials in one callback, so the star portal's material is declared by
the first `/materialfx`; until then the portal shows its Bedrock texture.

## What each effect should look like

- **Dissolve** (`materialfx:wraith`, cutout). The pale robed figure burns away in an irregular
  noise pattern and reassembles every four seconds. Where it is about to vanish a thin bright
  orange edge glows, visible in the dark; gone texels show the world behind it.
- **Hologram** (`materialfx:hologram`, additive). A translucent cyan projection: its silhouette
  edges, where the surface turns away from you, glow brightest (rim light from the surface normal
  and view direction), scanlines drift upward, and the image flickers slightly. Walking around it
  moves the rim with the view.
- **Energy flow** (`materialfx:energy`, opaque block). Molten veins run down the wall from dark
  crimson through orange to white-hot and flow continuously across neighbouring blocks; the hot
  veins emit light, so the wall glows at night.
- **Status and damage flash** (`materialfx:sentinel`, opaque). The grey armoured figure takes a
  pulsing poison green, burning orange or frozen blue with a faint glow; `flash` washes it white
  and fades. Neither recompiles: the client part only changes parameters.
- **Star portal** (`materialfx:portal`, opaque block). A dark violet void with three layers of
  stars that slide against each other as you move your head (parallax from the view direction)
  and drift slowly, over a swirling nebula. Stars and nebula emit; the face between them is dark.

## Tests

- `cargo test -p material-fx`: the showcase builds an artifact the Experience runtime loads, with
  both blocks, and a client part bundle the client verifies, with the `render_world_material`
  permission and the `materialfx` namespace.
- `cargo test -p cinnabar --lib material_fx`: the real client part runs in the real developer
  helper; its declarations commit, the client validates and publishes them within their budgets,
  a flash updates a parameter without a new shader, revoke restores every Bedrock material and
  restore binds them again.
- `cargo test -p render --test it material_fx`: each shader rastered on the GPU shows its
  distinguishing pixels (the burning edge and the cut, the rim and scanlines, the hot veins and
  their motion, the tint and flash, the stars' parallax).
- `cargo test -p render --lib scale_tests` and `many_bound_actors`: published materials request
  their pipelines before any bound block is in view, and 1 024 bound sections or 1 000 bound
  actors request no further pipelines.
- `go test -run MaterialFx .` in `tools/localserver`: the pack declares each entity and the
  commands become the client part's control records.
