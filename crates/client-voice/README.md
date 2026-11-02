# client-voice

Proximity voice chat. A player opens a room on their machine and shares a room code; friends
paste it to join. Audio goes player to host over UDP, and the host's relay forwards each frame
only to players within 48 blocks in the same dimension.

- `engine::VoiceHandle` is the whole API: `spawn`, `set_local_player`, `host_room`,
  `join_room`, `leave`, mute/deafen/push-to-talk, volumes, and `snapshot` for the interface.
- `device::CpalIo` is the microphone and speaker backend (WASAPI on Windows, Oboe on Android).
- `adpcm` is the codec: 16 kHz IMA ADPCM, about 65 kbit/s, no native dependencies.

Status: the core and the session engine are tested with fake audio devices. `device.rs` and
the client wiring (a `VoiceHandle` resource, the pose feed and the interface) are not
compiled or tested yet.

Limits: audio is not encrypted, the host must expose UDP port 24454, and the code contains the
host's IP address.
