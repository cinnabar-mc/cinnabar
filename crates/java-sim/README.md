# java-sim

Local-player physics for Java Edition 1.7.10 servers, in the game's own arithmetic. The crate
is independent of the Bedrock simulator in `sim`: a caller supplies a `World` of 1.7.10 block
ids and metadata, feeds one `Input` per tick, and reads back the position and the movement
packets the 1.7.10 client would send.

- `player`: the client tick (input, sprint and flight state, jumping, water, lava, ladders,
  cobwebs, soul sand, block collision with stepping and sneak edges) and the sprint, sneak,
  position and look reports that follow it.
- `block`: material facts and collision boxes for block ids 0 to 175, including the
  neighbour rules of fences, walls, panes, stairs, doors and chests.
- `math`: the sine table, flooring and the axis-aligned box.

## Fixtures

`fixtures/` holds recordings of the official 1.7.10 client (`client.jar`, SHA-1
`e80d9b3bf5085002218d4be59e668bac718abbc6`) run headlessly: its real client player class
ticked in a real client world, with server events delivered through its packet handlers and
the packets it queued read back. The recorder and the game files are not part of this
repository. Floating-point values are stored as raw bit patterns and the tests require exact
equality.

| File | Content |
| --- | --- |
| `blocks.jsonl.gz` | Material and shape-class facts for each registered block id. |
| `shapes.jsonl.gz` | Collision boxes for every id and metadata alone, and for neighbour layouts of connecting blocks. |
| `sin.json` | Hash and samples of the sine table. |
| `traces/*.jsonl.gz` | Per-tick input, server events, resulting state and sent packets for scripted scenarios and three seeded 3,000-tick random walks over an obstacle course. |

A trace starts with a header line (blocks, start position, abilities) and continues with one
line per tick: `ev` (events applied before the tick), `in` (keys and view angles) and `out`
(position, motion, flags and packets).

## Not modelled

- Riding, sleeping and death animations; the caller blocks movement input instead.
- Moving pistons, whose collision depends on tile-entity state, and entity boxes unless the
  caller supplies them through `World::entity_boxes`.
- Anvils, chests and ladders with metadata outside 2 to 5 collide in the game with whatever
  bounds were last computed for any block of that kind. The crate computes each block's own
  bounds; ladders with other metadata collide as a full cube.
- Attribute modifiers of one operation combine in Java 8's hash order. Older runtimes may
  differ in the last bit of the product.
