# Particle tint and cutout coverage

The pack pinned by `assets/vanilla-source.json` documents particle materials in
`metadata/doc_modules/particles.json`, under Materials. `particles_alpha` draws
colored sprite pixels opaquely and uses sprite alpha for cutout coverage.
`particles_blend` and `particles_add` use tint alpha for their respective blend
modes.

The pinned `particles/redstone_wire_dust.json` and `particles/explosion_level.json`
both author RGB tint gradients with alpha zero and use `particles_alpha`. These
are valid visible effects: the draw-list builder must not discard them because
their tint alpha is zero. It publishes opaque tint alpha for this material while
retaining RGB, billboard geometry, lifetime and the sprite alpha-test flag. The
shader continues to discard transparent sprite texels. Blended and additive
materials retain their authored tint alpha, including fully transparent particles.

This rule applies equally to named particle requests and legacy level events;
it does not depend on a server resource pack or an effect-name exception.

Synthetic regressions cover fixed and gradient colors, zero and partial alpha,
initial bursts with immediate emitter expiry, and particle retirement. The
optional `CINNABAR_CUTOUT_PARTICLE_FIXTURE` test accepts an absolute path to a local
effect definition and verifies named spawning through the draw list. The pinned
redstone dust and explosion definitions pass that check. Matched in-game frames
for redstone trails/rings and white cloud particles remain required; simulation
and draw-list checks do not qualify their complete appearance or motion.
