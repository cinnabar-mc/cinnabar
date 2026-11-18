# Independently pitched sound voices

Server `PlaySound` packets can reuse one instrument definition at different pitches.
Each request needs its own playback cursor. A shared decoded sample does not make
these requests duplicates: a fast melody can overlap many tails of the same
instrument even when each chord contains only a few notes.

The previous six-voice cap counted only the definition name, ignoring pitch and
start time. Its pending-decode counterpart also discarded starts sharing a sample
path. Both rejected valid notes before the overall voice budget was reached.

The engine now admits each start independently, including while sharing one
background decode. The existing overall voice, pending-start and request-queue
budgets still apply. Their native limits, voice-stealing policy and timing remain
unqualified; this correction does not close the broader audio parity gate.

Original synthetic regressions play fourteen pitches through one warm sample and
through a cold shared decode. They check admission, independent source lengths and
unchanged pitch, plus absence of discarded starts. They contain no game audio or
server song payloads.

After rebuilding, compare a server melody with vanilla at the same location,
category volume and effect phase. Listen to the first cold play and repeated
plays for missing notes and interrupted tails. Matched live listening remains pending.
