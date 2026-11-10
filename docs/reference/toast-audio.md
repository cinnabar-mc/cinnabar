# Notification audio

`ToastRequest` carries the notification title and body, without a separate sound
request. A server can rely on the client to play `random.toast` when presenting it.
Showing only the text leaves these notifications silent even with a complete
sound bank. Explicit `PlaySound` routing is a separate path.

Finite HUD toasts now emit the named interface cue once when they reach the front
of the presentation queue. Waiting toasts do not sound early, repeated frames do
not repeat it, and expired or retired session notifications cannot sound later.
Standing client notices retain their existing behavior. The cue uses the ordinary
audio catalog, server overrides, category volumes and background sample decoder.

Synthetic tests exercise received notifications, queue boundaries, expiry and
session replacement. An optional mixer test reads `CINNABAR_TEST_AUDIO_CATALOG`
and `CINNABAR_TEST_SOUND_BANK` in place, checking that the installed sample produces
nonzero output. No game audio is included in these tests.

The reference scenario is a Bedrock 1.26.60 server achievement notification.
Matched live listening, precise native animation-to-sound timing and the behavior
of other native toast types remain unqualified; this change does not close the
HUD or audio parity gates.
