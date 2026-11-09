//! Server WIT 0.12's containers: what a callback reaches, reads and stages, and the snapshots
//! that are malformed.

use super::*;
use crate::protocol::Container;

/// The other half of a double chest listed at [`UP`]: above [`EAST`], so in reach only through a
/// network holding it.
const CHEST_PAIR: BlockPos = BlockPos { x: 1, y: 65, z: 0 };
/// Next to [`EAST`] alone.
const BEYOND: BlockPos = BlockPos { x: 2, y: 64, z: 0 };
/// Two blocks above the anchor, out of every reach here.
const OUT: BlockPos = BlockPos { x: 0, y: 66, z: 0 };
/// Beside both [`EAST`] and [`SOUTH`], so a chest half either could pair with.
const SOUTH_EAST: BlockPos = BlockPos { x: 1, y: 64, z: 1 };

fn stone(count: u8) -> ItemStack {
    ItemStack {
        id: STONE.to_owned(),
        metadata: 0,
        count,
        max_count: 64,
        data: None,
        plain: true,
    }
}

/// A loaded container at `pos` of `slots` slots, the first, if any, holding 10 stone.
fn container(pos: BlockPos, pair: Option<BlockPos>, slots: usize) -> Container {
    let mut contents = vec![None; slots];
    if let Some(first) = contents.first_mut() {
        *first = Some(stone(10));
    }
    Container {
        pos,
        loaded: true,
        pair,
        slots: contents,
    }
}

impl Fixture {
    fn containers(mut self, blocks: Vec<Container>, truncated: bool) -> Self {
        self.containers = Some(Containers { blocks, truncated });
        self
    }

    /// Makes [`EAST`] an owned counter in the anchor's network.
    fn networked(mut self) -> Self {
        self = self.cell(EAST, COUNTER, true, None);
        self.network = Some(Network {
            blocks: vec![ANCHOR, EAST],
            truncated: false,
        });
        self
    }
}

/// The anchor's interaction in a network with [`EAST`]: a double chest above both and an
/// unloaded position to the anchor's north.
fn chest() -> CallbackRes {
    let unloaded = Container {
        pos: NORTH,
        loaded: false,
        pair: None,
        slots: Vec::new(),
    };
    Fixture::new()
        .networked()
        .containers(vec![container(UP, Some(CHEST_PAIR), 54), unloaded], false)
        .res()
}

/// Both halves of a double chest read the same slots, and a slot written through one half
/// replaces the write through the other in one op at the listed half.
#[test]
fn a_double_chest_is_whole_at_either_half() {
    let mut res = chest();
    let slots = res.container(UP).unwrap().unwrap();
    assert_eq!(slots.len(), 54);
    assert_eq!(slots[0], Some(stone(10)));
    assert_eq!(res.container(CHEST_PAIR).unwrap().unwrap(), slots);
    assert_eq!(
        res.set_container_slot(CHEST_PAIR, 0, Some(new_stack(STONE, 4, None)))
            .unwrap(),
        Ok(())
    );
    assert_eq!(
        res.set_container_slot(UP, 0, Some(new_stack(STONE, 3, None)))
            .unwrap(),
        Ok(())
    );
    assert_eq!(res.set_container_slot(UP, 1, None).unwrap(), Ok(()));
    assert_eq!(
        res.container(CHEST_PAIR).unwrap().unwrap()[0],
        Some(stone(3))
    );
    assert_eq!(
        res.ops,
        vec![
            Op::SetContainerSlot {
                pos: UP,
                slot: 0,
                stack: Some(new_stack(STONE, 3, None)),
            },
            Op::SetContainerSlot {
                pos: UP,
                slot: 1,
                stack: None,
            },
        ]
    );
}

/// Out of reach is denied, a block in reach without a container is unknown, an unloaded one is
/// unavailable, a slot past the container's is out of bounds, and a stack the guest may not make
/// is refused as `set-slot` refuses it; none of them stages anything.
#[test]
fn reads_and_writes_outside_a_container_are_refused() {
    let mut res = chest();
    assert_eq!(res.container(OUT).unwrap(), Err(WorldError::Denied));
    assert_eq!(res.container(SOUTH).unwrap(), Err(WorldError::UnknownBlock));
    assert_eq!(res.container(NORTH).unwrap(), Err(WorldError::Unavailable));
    let below = BlockPos { y: -65, ..ANCHOR };
    assert_eq!(res.container(below).unwrap(), Err(WorldError::OutOfBounds));
    assert_eq!(
        res.set_container_slot(UP, 54, None).unwrap(),
        Err(WorldError::OutOfBounds)
    );
    assert_eq!(
        res.set_container_slot(UP, 0, Some(new_stack(STONE, 65, None)))
            .unwrap(),
        Err(WorldError::TooLarge)
    );
    assert_eq!(
        res.set_container_slot(UP, 0, Some(new_stack(HIDDEN, 1, None)))
            .unwrap(),
        Err(WorldError::UnknownBlock)
    );
    assert_eq!(
        res.set_container_slot(NORTH, 0, None).unwrap(),
        Err(WorldError::Unavailable)
    );
    assert_eq!(
        res.set_container_slot(OUT, 0, None).unwrap(),
        Err(WorldError::Denied)
    );
    assert!(res.ops.is_empty());
}

/// Truncated containers leave a position in reach that they do not list unavailable, not
/// unknown.
#[test]
fn truncated_containers_leave_the_rest_unavailable() {
    let mut res = Fixture::new()
        .containers(vec![container(UP, None, 27)], true)
        .res();
    assert_eq!(res.container(SOUTH).unwrap(), Err(WorldError::Unavailable));
    assert_eq!(res.container(UP).unwrap().unwrap().len(), 27);
}

/// The anchor's network extends the reach to its members' neighbours; without it the same
/// container is out of reach, which the adapter never sends.
#[test]
fn a_network_extends_the_reach() {
    let mut res = Fixture::new()
        .networked()
        .containers(vec![container(BEYOND, None, 5)], false)
        .res();
    assert_eq!(res.container(BEYOND).unwrap().unwrap().len(), 5);
    let error = Fixture::new()
        .containers(vec![container(BEYOND, None, 5)], false)
        .prepared()
        .err()
        .unwrap();
    assert!(error.contains("out of reach"), "{error}");
}

/// Snapshots the adapter never sends are malformed.
#[test]
fn malformed_containers_are_rejected() {
    let unloaded_with_slots = Container {
        loaded: false,
        ..container(UP, None, 27)
    };
    let mut bad_stack = container(UP, None, 27);
    bad_stack.slots[1] = Some(stone(65));
    let mut foreign_data = container(UP, None, 27);
    foreign_data.slots[1] = Some(ItemStack {
        data: Some("00".to_owned()),
        ..stone(1)
    });
    let cases = [
        (
            vec![container(UP, None, 27), container(UP, None, 27)],
            "listed twice",
        ),
        (
            vec![
                container(EAST, Some(SOUTH_EAST), 54),
                container(SOUTH, Some(SOUTH_EAST), 54),
            ],
            "listed twice",
        ),
        (vec![container(EAST, Some(UP), 54)], "not beside it"),
        (vec![unloaded_with_slots], "has a pair or slots"),
        (vec![container(UP, None, 0)], "slots"),
        (vec![container(UP, None, 55)], "slots"),
        (vec![bad_stack], "more than its most"),
        (vec![foreign_data], "not its own item"),
        (
            vec![container(UP, None, 1); crate::limits::MAX_CONTAINERS + 1],
            "the limit is",
        ),
    ];
    for (blocks, expected) in cases {
        let error = Fixture::new()
            .containers(blocks, false)
            .prepared()
            .err()
            .unwrap();
        assert!(error.contains(expected), "{error}: want {expected}");
    }
}
