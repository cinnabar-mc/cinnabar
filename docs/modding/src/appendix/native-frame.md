# Native frames for trusted WASI packet mods

This optional core-module ABI extends packet mods without changing their existing
`start`, `on-batch`, `tick` or `stop` exports. Components retain the existing relay ABI.

Export both `native-init(mask: i32) -> ()` and `frame(ptr: i32, len: i32) -> i32`.
After `_initialize` and before `start`, the host calls `native-init`. Bit 0 means
the host supplies player frames and accepts local camera deltas. It is set only
with both `players` and `camera` grants. Unrecognized bits must be ignored.

A reactor decoding the current SDK record selects it with `mod_api::current_native_frames!()`
at module scope, or by adding a `cinnabar:extension/native-frame-wire` custom section whose
payload is the single byte `2`. Unmarked built reactors retain their original wire record.
The drawing callback keeps its independent version 1 contract.

`frame` receives UTF-8 JSON matching `native_frame::Frame`, or `null` when gameplay
does not own input. Return the existing canonical `result<list<u8>, string>`:
discriminant at offset 0, list pointer at 4 and list length at 8. Successful bytes
contain JSON `null` or `native_frame::Delta`. An optional `cabi_post_frame(i32)`
releases the result. Input and output each have a 65536-byte bound.

The host owns input capture, snapshots and camera permission. Guest code owns its
feature policy. Calls run on the session's packet worker, serialized with relay
calls. Frames coalesce under load; the renderer never waits for the guest. Output
is consumed once and discarded after input or session ownership changes. Angles
are radians; positive yaw turns left and positive pitch turns up. This API adds no
packets and does not bypass the normal camera owner.

`native-frame-v2.json` is the shared cross-language conformance fixture. Treat its
64-bit IDs as integers, preserving all bits. SDK bounds derive from the existing
player and camera limits.

## General camera effects

A core module may additionally export `camera-frame(ptr: i32, len: i32) -> i32`.
Capability bit 2 (`4`) selects this callback when both `camera` and `movement` are
granted. Its input and canonical result ABI match `frame`. A connected world without
gameplay input carries `suspended: true` and no targets; the older
callback still receives `null`. Successful output is
`null` or `native_camera::Effects`. It takes precedence over the older `frame`
callback. Without a players grant the host removes player observations before
calling the guest. `cabi_post_camera-frame(i32)` may release the result.

Effects carry an optional once-consumed rotation, an optional absolute presented
`view` pose computed by the guest (`position` in world blocks, `yaw` and `pitch`
in actor YXZ radians, `velocity` in blocks per real second), `capture_look`,
`capture_movement`, an optional current-frame `view_tilt` (`roll` and `pitch`
degrees and `suppress_hurt_shake`), and teleport-view preservation. The `view`
pose belongs to the frame its callback observed. On every rendered frame the host
presents it with the position advanced by `velocity` for the real time since that
frame was sent, at most 250 ms; the body's pose, physics and network cadence are
unchanged. No effect changes a teleport destination or its acknowledgment.
`native-camera-v1.json` is the cross-language output fixture.

With `capture_look` and a `view`, native look input stops rotating the body. On
every rendered frame the host instead adds it to the presented view's yaw and
pitch, clamping pitch to ±89.9°, on top of the look already applied since the
observed frame was sent. The next delivered frame's `look_delta` reports exactly
the look applied since the previous delivered frame, so a guest that adds it to
its own yaw and pitch stays aligned with the presented view, with no jump when a
callback arrives. With `capture_movement`, which requires a `view`, the ordinary
movement, jump and sneak bindings stop moving the body and are still reported.

`view_tilt` adds degrees to the first-person view where the native damage shake
applies, and `suppress_hurt_shake` removes that shake; death rotation and view
bob remain. Positive pitch turns the view up. Positive roll turns it the way a
native shake without direction does: the view's right edge rises, so the world
appears to turn clockwise. A native shake of angle `a` from hurt direction `d`
is approximately roll `a·cos d` and pitch `-a·sin d`, peaking at 14° times the
player's damage bob.

Every frame also reports `look_delta` (yaw and pitch radians the host applied to
the captured view since the previous delivered frame, otherwise zero),
`move_axes` (the ordinary movement bindings, `+X` right and `+Y` forward, each
-1 to 1), the held jump and sneak bindings as `ascend` and `descend`, and
`elapsed` (real seconds since the previous delivered frame, 0 on the first frame
after a reset), so a guest can integrate a fly or orbit camera itself. Frames the
worker never received contribute their look and time to the next delivered one.

Presentation configuration remains valid until replaced or released; only relative
rotations expire after two client frames and are consumed once. Worker contention
does not reset the view. Packet updates invalidate unconsumed rotations. A connected suspended frame
retains only passive view-tilt and teleport-view policies; it releases the view
and look capture immediately. Revocation, dimension/session changes, failure and stop
invalidate every effect. Omission
restores the player's current camera settings and perspective.


### Retained input intents

Capability bit `64` additionally permits `look` and `movement` in camera effects.
`look` supplies absolute yaw/pitch in radians and a turn speed in radians per
second. The host evaluates it against its current pose on every render frame.
It excludes relative rotation. `movement` supplies unit
axes (`+X` right, `+Y` forward), their yaw frame, and held jump/sneak/sprint
requests. The host rotates these axes after camera updates and feeds its ordinary
semantic input and physics; physical movement takes priority. Resolved held
requests preserve the player's separate toggle settings. Input loss releases controls and fences older callbacks;
unchanged suspended frames still permit world drawing. Retained input expires
250 ms after its observation. Session/world replacement, revocation, failure and
stop release it as well. Guests must omit these fields without bit `64`.

## World drawing

The independent `render-frame` callback uses native-init bit `8` when both
`world_draw` and `render_through_walls` are explicitly granted. Its canonical ABI,
optional `cabi_post_render-frame` and serialization match the other frame callbacks.
Input remains bounded to 64 KiB; complete drawing output is bounded to 256 KiB. Its JSON input is `native_render::Frame`: version, session
and dimension only, or `null` outside a current world. Drawing does not require
camera/movement/player observation and continues while gameplay input is suspended.

Successful output is `null` or a complete `native_render::Scene` with the matching
session/dimension. It contains at most 64 oriented boxes and 128 world lines, using
existing retained-drawing colour and coordinate bounds. The renderer reuses unit
geometry and instance handles and splits long lines into bounded retained pieces.
A line exceeding the remaining per-owner instance budget is omitted whole.
Optional `entities` groups select actual animated actor meshes with a validated
`MaskStyle`; native-init bit `16` additionally requires `entity_observe` and
`entity_highlight`. Up to eight styles share the existing actor-mask compositor,
with at most 32 actors per group and the existing vertex/attachment budgets.
When attachments exceed the GPU budget, fitting groups remain visible. Admission
offers each owner one coverage group before its next, then follows stable owner
and handle order. This also applies to component masks sharing the compositor.
Optional `blocks` contains at most 1024 integer positions and per-block colours;
bit `32` requires `block_highlights`. These use the native full-block overlay. The independent consent bits do not
change which ordinary box/line callbacks are admitted. Unknown or removed actor
incarnations cannot keep an old mesh selected.
Retained geometry and depth permissions are admitted by the same drawing store
used by component extensions. Invalid wire output quarantines its producer;
resource-admission failure clears its scene atomically. World loss, revocation,
callback failure and stop release every drawing. Unchanged scenes preserve their
resource identity. The shared `native-render-v1.json` fixture records the wire ABI.
