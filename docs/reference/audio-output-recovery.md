# Output stream recovery

An endpoint can remain the system default after its current output stream fails.
Comparing device identities alone cannot detect this condition. In the pinned
rodio 0.20.1 transport, the error callback only prints; CPAL 0.15.3's ALSA worker
continues polling after a descriptor error. A persistent failure can therefore
flood stderr while playback remains attached to the failed stream.

The desktop transport opens CPAL directly and retains the first callback error
in a transport-owned latch. The callback performs no logging and stores no error
history. The audio owner observes the latch at its existing device-check interval,
logs once for that failed stream, drops it and reopens the current default endpoint.
Opening failures use the existing retry path. Each format attempt and replacement
has a separate latch, so a retired callback cannot invalidate a healthy replacement.
The selected endpoint, format fallback order and sample conversion remain those
used by the previous rodio transport.

The persistent mixer outlives the hardware stream. Active playback cursors and
pending sources remain owned by it during recovery; cancellation still applies.
The old callback must release its mixer lease before a replacement attaches it.
This preserves remaining source samples, but cannot recover audio already submitted
to a failed hardware buffer or guarantee gapless playback.

Synthetic tests cover failure without an endpoint change, unsuccessful reopening,
continued active and pending audio, repeated error coalescing, late old-callback
errors and signed, unsigned and floating-point output buffers. They require no
audio device or game resources. The same-endpoint failure and retry regressions
failed before the recovery change.

The contributor reproduced repeated ALSA descriptor errors after dense sound effects
and confirmed recovery with this fix on the affected machine. The initial backend
failure cause remains undetermined. Recovery on other devices and platforms, the
polling cadence and native recovery timing remain unqualified.
