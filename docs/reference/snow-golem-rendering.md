# Snow-golem arm cube binds

The vanilla 1.26.60.4 resource archive declares separate cube bind rotations for
the modern snow golem's two arms: arm1 `[0, 0, 57.3]`, arm2 `[0, 180, -57.3]`.
The pinned sample omits both. Without them the identical arm cubes overlap on
one side of the body.

The existing content-pinned bind restoration in the entity compiler restores
these two values. They affect cubes independently of animated bone rotations,
parenting and locators. The correction requires the exact sample path, geometry
identifier and SHA-256; custom sources, explicit binds and the older animated
snow-golem geometry retain their authored transforms.
An unconditional test requires reviewing this correction when the target or
sample commit changes.

Tests use original synthetic geometry to check both arms, unchanged animation
frames and explicit overrides. `CINNABAR_VANILLA_ROOT` optionally checks the
installed sample in place, including exclusion of changed payloads. No game
resource payload is included in the fix or tests.

After rebuilding with `make play`, inspect a snow golem from both sides and
while its body turns. Confirm separated arms, correct joins to the torso,
lighting and depth. Matched gameplay frames remain pending.
