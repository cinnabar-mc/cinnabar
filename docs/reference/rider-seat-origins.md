# Rider seat origins

The actor store retains collision feet. Player native actor positions use a
vertical origin offset. Authored seat coordinates need the passenger's riding
height before they can join those native origins.

An authored base `minecraft:rideable` seat is multiplied by the mount's scale,
then the passenger's riding height is added to Y. For an adult player, riding
height is its native origin offset plus the sitting adjustment; the baby flag
suppresses that adjustment. Streamed `SeatOffset` metadata already includes
these adjustments and is used directly. Both resolved sources then use the
native mount origin as the anchor and remove the rider's origin offset once
to recover collision feet. Seat selection is retained.
Unknown or unadmitted seats still retain the streamed rider position.

An unlocked player rider continues to turn its body with its look during the
same ride. Riding suppresses the walking cycle, while the ordinary player body
follow still runs. The look comes from head yaw because a remote passenger's
actor facing may be retained from its mount. An explicit seat-facing lock still
overrides the body follow after the motion update.

The [rideable component reference](https://learn.microsoft.com/minecraft/creator/reference/content/entityreference/examples/entitycomponents/minecraftcomponent_rideable)
defines a seat's position relative to its entity. The version-specific native
origin convention is retained by the actor store's `network_position_offset`.

An original synthetic regression supplies an authored minecart seat and its
resolved streamed offset to a player, checking local queries, predicted poses
and committed passenger placement against the expected feet. It covers zero
and nonzero mount scales, the baby flag, and links arriving before or after actors.
Existing streamed-seat tests cover updates, rotation and dismounts.
Synthetic animation tests cover local and remote players turning in place during
one unlocked ride, idle gait while seated, and an explicit seat-facing lock.

The reported scenario is a Bedrock 1.26.60 server bench implemented with an invisible
minecart. Matched live camera/body placement and special passenger families
still need qualification; this does not close movement or rendering parity gates.
