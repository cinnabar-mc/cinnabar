# Browser spectator streams

The browser client can watch a read-only spectator stream instead of joining a server through
the relay. A host service sends an ordinary Bedrock session to the same game owners: terrain,
actors, equipment, first-person hands, HUD, nametags, particles and sounds all go through the
normal client. The client has no account, relay control channel or play Hello in this mode.

## Page entry

`build-web` stages `spectate.html` with the regular page. Open it with:

| Query | Meaning |
| --- | --- |
| `assets` | HTTPS directory, ending in `/`, that holds the bundle's `manifest.json` and carriers |
| `stream` | HTTPS URL of the spectator stream |
| `parent` | Optional HTTPS origin of an embedding page |

Plain HTTP is accepted only on loopback hosts, for local development; sound still needs an
HTTPS asset directory. Same-origin asset requests carry credentials, so a host may keep the
bundle behind its own session. The page forwards no
keyboard or mouse input. A click resumes audio. When `parent` is set, the page posts each
state or error notice to that origin as `{type: "cinnabar-spectator-notice", state | error}`.
Startup failures are reported the same way. The parent window itself may switch streams with
`{type: "cinnabar-spectate", stream}`, including while the client loads; messages from any other
window or origin are ignored. Pages that embed the client directly call `Game.spectate(url, observe)`.

## Stream contract

The stream URL is opened as a WebSocket with the relay subprotocol (`cinnabar.relay.v2`). Its
bytes are the session endpoint framing from `crates/protocol/src/session_wire.rs`, without the
relay's Hello:

1. The client sends one Connect frame.
2. The host answers with a Handoff whose startup packets end with StartGame, preceded by the
   item registry. Handoff packs need `sha256`. Their archives follow as PackData frames
   (at most `PACK_CHUNK_BYTES` each), before any batch.
3. The host sends Batch frames. The usual spawn prerequisites apply (`ChunkRadiusUpdated`,
   `PlayStatus` PlayerSpawn and terrain).
4. A Disconnect frame ends the stream with a visible message. Transfers are refused.

The client gives up when the socket does not open within 15 seconds or sends no bytes within
20 seconds of opening.

Client packets (movement, loading and initialization) still arrive. Spectator hosts read and
discard them. WebSocket messages may split or join frames, as on relay play channels. Browser
receive limits apply: at most `RELAY_PLAY_MESSAGE_BYTES` per message and in total unread,
and at most `QUEUED_FRAMES` unread messages. Hosts should coalesce small frames and pace bulk
data such as packs and terrain.

## Camera

The local player is the spectator. A host typically:

- keeps it in spectator mode, immobile and invisible, under the `minecraft:free` camera preset,
  and moves the camera with eased `CameraInstruction` set instructions; or
- clears the camera instruction and moves the local player (survival, immobile) with
  MovePlayer. In that mode its own hands, hotbar, health, effects and HUD show the attributes
  and inventory the host sends.

Titles, action bars, popups and sidebars render in both modes.
