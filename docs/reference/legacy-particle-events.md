# Legacy particle level events

The active target is recorded in `assets/bedrock-target.json`. Its particle selectors
are pinned in `crates/protocol-ids/data/particle-types.json`, including the source
metadata hash and native-enumeration reference. The pinned sample schema uses explicit
`x-enum-binary-value` entries; array positions are not wire IDs. Its command vocabulary
omits `MobPortal` (24) and aliases `WindExplosion` to 90; the native selectors distinguish
`VaultConnection` (90) from `WindExplosion` (91).

Gophertunnel's particle constants predate the insertion of `CandleFlame` (9) and
`BreezeWindExplosion` (18), and place some newer entries in a different order. They
must not supply selectors for the active target. `registry-policy.json` names particle
types; `registryc --kind events` resolves them through the shared protocol contract.
Parameter decoders use that same contract for terrain, item icons, snowball and slime.

For particle level events, the low selector bits choose the effect. `FallingDust` (33),
`MobSpell` (34), `MobSpellAmbient` (35) and `MobSpellInstantaneous` (36) carry packed ARGB
color. Requests bind both the spell-color queries and `variable.color` channels without
changing the server position. The selected pack definition owns emission shape, motion,
billboard artwork and lifetime. A dust trail therefore uses the point-shaped dust
definition instead of a spell emitter's volume, and an instantaneous spell keeps its
color instead of selecting slime item fragments.

Regressions pin selectors around insertion and reordering boundaries, preserve item and
terrain payloads, and carry colored requests through simulation into the draw list.
Matched post-change dust-trail and spell-rainbow frames remain required. These numeric
routing checks do not qualify every effect's native emission, timing or artwork.
