# Local worlds

Single-player worlds run on a local server behind the same core and session endpoint as online play.
The launcher (`crates/launcher-host/src/local_worlds`) owns the world store, the BDS download and the
server processes; the core only routes sessions to the open world. Full scope: `plan.md` Phase 7.

## Local server

Every new or imported world uses Minecraft's official Bedrock Dedicated Server (BDS).
Infinite and Flat terrain use the same host. Creation waits for runtime detection and refuses
unsupported hosts. Accepting the Minecraft EULA is required before downloading or opening BDS.

| Runtime | Where |
| --- | --- |
| Native BDS | Windows and Linux x86-64 |
| Container BDS | Desktop hosts with a Docker-compatible runtime |
| Translated BDS | Android ARM64 with the bundled runtime; device acceptance remains incomplete |

The exact server version and container image come from `assets/bedrock-target.json`.
`docker_missing` and `docker_not_running` are reported in world status. Android acquires the
Linux server and pinned guest runtime on first use and does not offer Docker installation.
See [Android](android.md) for the remaining device and lifecycle acceptance work.

### Saved Dragonfly worlds

Worlds tagged `dragonfly`, including older records with no backend field, remain in the list
with a **Conversion required** label. Opening one reports that BDS cannot open it yet and leaves
its files unchanged. Renaming and deleting remain available. There is no conversion or fallback
server in this version. On desktop, use Settings → Storage to open the local-worlds folder and
copy a backup before using an external conversion tool. Android keeps these saves in the app's
private storage; in-app export and conversion are unavailable. Compatibility of converted chunks,
actors and player data is unverified.
Existing BDS worlds continue to open normally.

## World options

General settings select the name, game mode and difficulty. Advanced, Multiplayer and Cheats
use the shared option catalog in `launcher::local_worlds::options`. The catalog owns the labels,
defaults, bounds, cheat gating and BDS destinations. Changing a cheat setting enables cheats;
turning cheats off restores the gated rules' defaults.

World metadata stores individual pending edits and inactive preferences. After a successful open,
the world file supplies the current rules, including changes saved during play. Before BDS starts,
the host updates the owned fields in `level.dat` and preserves other tags. Game rules, starting map,
bonus chest, sleep settings,
respawn radius and daylight settings use level data. Server properties apply cheats, simulation
distance and default player permissions to both native and container servers.
Like a vanilla world owner, the signed-in host is an operator through the install's
`permissions.json`, keyed by XUID; friends and other joiners get the default permission. A start
made while the account's profile loads starts native or translated BDS immediately, then waits
for the host's operator grant before allowing a join. The existing profile-load deadline runs
alongside server startup; container servers still resolve the host before launching.
A signed-out host has no XUID, so it keeps the default too.
Turning multiplayer off limits the server to its host and withdraws friend hosting.
LAN visibility is not offered: both runtimes bind and advertise loopback only, so discovery stays off.
A new world's first start lets BDS create it from server.properties and stops it once ready, so its
own new-world setup (spawn search, world version, flat layers) runs; the host then patches only
owned fields of that level.dat and starts it again. The host never writes level data BDS has not
created.
Reopening waits for the prior server to finish saving before it reloads rules and applies pending edits.
Seed and terrain apply during creation; starting map and bonus chest are also fixed after creation.
The edit screen exposes the remaining options.
Older and imported worlds recover their options from level data. A `level.dat` over 1 MiB is
never parsed or rewritten; that world's options show as unavailable.

While the host plays a BDS world, Settings lists a Game section under World in vanilla's in-game
order and wording. Name, seed, world type, starting map, bonus chest, simulation distance and
hardcore stay locked; cheat rules wait for cheats, and turning cheats off keeps them. Each change
saves at once. Live changes reach the running server through its console (`gamerule`, `difficulty`,
`alwaysday`, `changesetting allow-cheats`); container BDS takes the line through the server's stdin.
Default game mode and default player permission apply on the next start. If a console write fails,
the Game section explains that the saved change applies on the next start. Rules applied live stay
pending until the server exits successfully. A crash or forced shutdown keeps them pending.

Resource packs, behaviour packs and experiments remain unavailable. Xbox Live, Realms and
friends-broadcast settings are not exposed on the local BDS multiplayer page. Hardcore remains
unavailable, as do custom flat presets and Education Edition. Runtime acceptance of every option across supported BDS platforms remains open.

## BDS acquisition

Never bundled or committed. `bds/` beside the worlds dir holds `<version>/` builds with a
`manifest.json` (URL, zip SHA-256, size, platform, time).

- The build is `server_version` in `assets/bedrock-target.json`, fetched from its versioned official URL.
  Without it nothing is downloaded.
- Only https `minecraft.net` / `minecraft-services.net` hosts (including redirects) are accepted; zips are
  size-capped and unpacked with path-escape checks. Mojang publishes no hash, so the SHA-256 is provenance, not a pin.
- The EULA gate: opening a BDS world fails with `EulaRequired` until the user accepts the EULA; nothing is
  downloaded before then. Status carries `setup` (state, bytes, runtime, reason, `eula_accepted`).
- Container runtime: the launcher downloads the Linux build on the host (with byte progress) and mounts it as
  `/data`; `bedrock_server-<version>` beside it makes the image skip its own download. The image is
  `bds_container_image` in the manifest (a tag plus sha256 digest); an image without a digest is refused.
- Open stages in `setup.state`: `checking_runtime` (`docker info`), `pulling_image` (`layers_done/total`),
  `downloading` (`bytes_done/total`), `unpacking`; the client shows each on the loading screen.

## Runtime behavior

- **Storage:** `<worlds>/<id>/world.json` plus `db/` (a Bedrock world folder: `level.dat` + LevelDB) and `players/`.
  BDS sees `db/` through a directory link at `worlds/<id>` (junction on Windows) or a bind mount, so one world
  runs at a time. Legacy Dragonfly folders are preserved and never opened by BDS.
- **server.properties / env:** name, gamemode, difficulty, seed, level type, `online-mode=false`,
  the multiplayer player limit, `view-distance` at the client's largest render distance (each
  RequestChunkRadius picks the radius), independent simulation distance (4–12), `transport=nethernet`
  and `enable-lan-visibility=false`. Seed and level type apply at creation only.
- **Downloads** name the agent `Cinnabar-local-worlds`; minecraft.net resets unnamed downloaders.
- **Exposure:** native BDS listens on a random port with offline authentication and the configured player limit.
  The container publishes its transport and enabled discovery ports on `127.0.0.1` only.
- **Lifecycle:** ready on "Server started."; stop is `docker stop` (container) or `stop` on stdin, then kill after 30 s.
  Server processes are tracked children: on exit the client asks the open world's server to stop its
  own way (`stop`, or `docker stop`), cancels a start in progress, and gives the server its 30 s save
  budget before the exit sweep escalates to SIGTERM and kill.
- **World lifetime:** saved worlds start only on an explicit open. Leaving requests a save and
  stop immediately on every runtime, including translated BDS. Reopening waits for the previous
  server's save and backend cleanup before starting another server. Android setup prepares
  reusable startup code in a temporary world; attaching the launcher never starts a saved world.
- **Pause:** BDS worlds keep running while a menu is open or focus is lost;
  status reports `pause_supported: false`.
- **Login:** signed in, the core presents the account's identity. Signed out, it presents a self-signed NetherNet
  identity (BDS refuses anonymous HTTP offers) and an offline login from the client's identity, both admitted
  because `online-mode=false`. The launcher's own loopback BDS retries once with local identity if account
  services fail before the transport opens or the server reports missing account verification keys.
  Other server rejections and transferred or external servers never trigger this retry.
  The local retry has no account XUID or operator grant; player-data continuity between account-backed
  and local identities still needs acceptance. Player-data persistence needs a stable client identity.
- **Offline:** the client includes loopback ICE candidates for both local HTTP and LAN joins, so
  its transport can use same-device routes with external interfaces down. This does not create a
  missing server route: native BDS gathers ICE candidates only from non-loopback interfaces and has no other
  transport, so on a device with no network its SDP answer carries no candidate. The core fails such
  a join at once with `cinnabar.disconnect.localWorldOffline` rather than waiting for a candidate.
  The Android translator supplies an additional route over the existing loopback interface and
  passes offline transport, signed-out login and spawn in the emulator sandbox. This does not
  change native desktop BDS. Initial Android server/runtime downloads require a connection;
  full-APK and physical-phone offline acceptance remain open.

## Core route

The launcher reports the open world to the core with `local_world.v1`: `state` (`idle`, `starting`,
`running`, `stopping`, `failed`), `world` (`id`, `name`, `game_mode`, `backend`), `target` while running
(`address`, `transport` = `raknet` | `nethernet` | `nethernet-lan`, `lan_address`, `level_name`) and a path-free
`error` while failed. The menu treats a world as ready only once the core has acknowledged the running server's exact route. A session waits while the world starts, fails while it stops or has failed, and falls
through to other routes only when idle. A world that begins opening clears any `connect.v1` selection and
pending transfer. When multiplayer is enabled, the core hosts a running BDS world to Xbox friends
and serves `world_invite.v1` for it.

## Client

`crates/launcher/src/local_worlds` owns the screen model (list, create, templates, edit, delete, open stages,
EULA, Docker modal); `crates/client-shell/src/local_worlds` runs its requests against the launcher's world
manager. Play and the OreUI create/edit screens bind to it.

The Edit world screen saves changes before Play starts the server and when Back or Escape leaves
the screen. Invalid names stay in the form for correction; failed saves keep the edits for retry.
The selected game mode applies to new and returning players. BDS launches use `force-gamemode=true`
(including `FORCE_GAMEMODE` in containers), so existing worlds use the selected mode too.

## v1 limits

Full local-world parity and platform acceptance remain open. Docker-mounted LevelDB on macOS
can be slow. A killed client can leave a container running; the next start of that world removes it
before reserving ports.

## Developer servers

`tools/localserver` and its `cinnabarext` library remain available to server operators and mod
authors. They are never shipped or selected by the launcher. Run `make local-server` in a source
checkout to build the development server and Experience tools. `cinnabar-cxb mod run` builds its
own checkout server under `target/mod-tools`; paired-mod testing does not require a player package.
