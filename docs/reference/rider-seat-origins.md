# Rider seat origins

The actor store retains collision feet. Player native actor positions use a
vertical origin offset, so placing player feet directly at an authored seat puts
the body and its following camera too high.

Both seat sources now use the same conversion: a streamed `SeatOffset`, or a
base `minecraft:rideable` seat resolved from the entity carrier. The native mount
origin is the anchor, and the rider's origin offset is removed once to recover
collision feet. Seat selection and existing scale handling are retained.
Unknown or unadmitted seats still retain the streamed rider position.

An unlocked player rider continues to turn its body with its look during the
same ride. Riding suppresses the walking cycle, while the ordinary player body
follow still runs. The look comes from head yaw because a remote passenger's
actor facing may be retained from its mount. An explicit seat-facing lock still
overrides the body follow after the motion update.

The [rideable component reference](https://learn.microsoft.com/minecraft/creator/reference/content/entityreference/examples/entitycomponents/minecraftcomponent_rideable)
defines a seat's position relative to its entity. The version-specific native
origin convention is retained by the actor store's `network_position_offset`;
behavior-pack coordinates are not collision-foot coordinates.

An original synthetic regression supplies equivalent authored and streamed seats
to a player, checking both the local seat query and committed passenger placement.
It covers zero and nonzero mount scales and links arriving before or after actors.
Existing streamed-seat tests cover updates, rotation and dismounts.
Synthetic animation tests cover local and remote players turning in place during
one unlocked ride, idle gait while seated, and an explicit seat-facing lock.

The reported scenario is a Bedrock 1.26.60 server bench implemented with an invisible
minecart. Matched live camera/body placement, authored seat scaling and special
passenger families still need native qualification; this does not close movement
or rendering parity gates.
