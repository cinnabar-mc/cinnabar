//! Server WIT 0.10's overlays: `set-overlays` and `overlays`, and the overlay an interaction's
//! click picks.

use super::super::Export;
use super::*;
use crate::host::cinnabar::experience_server::types as wit;
use crate::protocol::{
    Condition, Overlay, OverlayDef, OverlayLook, Permutation, Pixel, PixelBox, QuarterTurns,
    StateTest, Visual,
};
use block_transform::overlay_face::MAX_CELL_OVERLAYS;

/// An overlay host of the Experience.
const BUS: &str = "probe:bus";
/// An overlay type: a 2-pixel plate against north, or with `probe:look` `stub` a stub box
/// turned a quarter about z.
const ARM: &str = "probe:arm";
/// An overlay type without a selection box.
const BARE: &str = "probe:bare";

fn pixel(x: f32, y: f32, z: f32) -> Pixel {
    Pixel { x, y, z }
}

fn pixel_box(min: [f32; 3], max: [f32; 3]) -> PixelBox {
    PixelBox {
        min: pixel(min[0], min[1], min[2]),
        max: pixel(max[0], max[1], max[2]),
    }
}

fn visual(selection: Option<PixelBox>) -> Visual {
    Visual {
        geometry: None,
        materials: Vec::new(),
        bones: Vec::new(),
        collision: None,
        selection,
        rotation: None,
    }
}

fn look_is(value: &str) -> Condition {
    vec![vec![StateTest {
        state: BlockState {
            name: "probe:look".to_owned(),
            value: StateValue::Choice(value.to_owned()),
        },
        equal: true,
    }]]
}

/// The Experience: probe:counter, which carries no overlays, [`BUS`], which does, and the
/// overlay types [`ARM`] and [`BARE`].
fn catalog() -> Catalog {
    let arm = OverlayDef {
        id: ARM.to_owned(),
        states: vec![
            StateDef {
                name: "probe:lit".to_owned(),
                values: StateValues::Bool,
            },
            StateDef {
                name: "probe:look".to_owned(),
                values: StateValues::Choices(vec!["plate".to_owned(), "stub".to_owned()]),
            },
        ],
        visual: visual(Some(pixel_box([0.0; 3], [16.0, 16.0, 2.0]))),
        permutations: vec![Permutation {
            when: look_is("stub"),
            geometry: None,
            materials: None,
            bones: None,
            collision: None,
            selection: Some(pixel_box([0.0, 6.0, 0.0], [4.0, 10.0, 6.0])),
            rotation: Some(QuarterTurns { x: 0, y: 0, z: 1 }),
        }],
    };
    let bare = OverlayDef {
        id: BARE.to_owned(),
        states: Vec::new(),
        visual: visual(None),
        permutations: Vec::new(),
    };
    Catalog {
        blocks: [
            (COUNTER.to_owned(), Vec::new()),
            (BUS.to_owned(), Vec::new()),
        ]
        .into(),
        items: [(CELL.to_owned(), 1)].into(),
        server: [server_item(STONE, 0, 64, false)].into(),
        overlay_hosts: [BUS.to_owned()].into(),
        overlays: [(ARM.to_owned(), arm), (BARE.to_owned(), bare)].into(),
        ..Catalog::default()
    }
}

/// An interaction with the bus at [`ANCHOR`], carrying `overlays`, clicked at `hit`; the
/// counter sits east of it and air elsewhere.
fn request(overlays: Vec<Overlay>, hit: Option<Pixel>) -> Request {
    let mut cells = vec![Cell {
        overlays,
        ..loaded(ANCHOR, BUS, true, None)
    }];
    cells.push(loaded(EAST, COUNTER, true, None));
    for pos in [UP, DOWN, WEST, NORTH, SOUTH] {
        cells.push(loaded(pos, AIR, false, None));
    }
    Request::Callback {
        seq: 1,
        info: Info {
            context: None,
            simulation: None,
            world_id: "world".to_owned(),
            dimension_id: "overworld".to_owned(),
            tick: 1,
            event_sequence: 1,
        },
        actor: Some(ACTOR.to_owned()),
        world_min_y: -64,
        world_max_y: 319,
        data_budget: 1 << 20,
        snapshot: cells,
        network: None,
        inventory: None,
        containers: None,
        call: Call::Interact {
            player: ACTOR.to_owned(),
            pos: ANCHOR,
            face: Face::North,
            hit,
        },
    }
}

fn res(overlays: Vec<Overlay>) -> CallbackRes {
    prepare(&Arc::new(catalog()), &request(overlays, None))
        .unwrap()
        .0
}

/// The slot an interaction clicked at `hit` targets, the bus carrying `overlays`.
fn target(overlays: Vec<Overlay>, hit: Pixel) -> Option<u8> {
    let request = request(overlays, Some(hit));
    let (_, export) = prepare(&Arc::new(catalog()), &request).unwrap();
    let Export::Interact { target, .. } = export else {
        panic!("an interaction prepares an interact export");
    };
    target
}

fn arm(slot: u8, face: Face, look: &str) -> Overlay {
    Overlay {
        slot,
        face,
        look: OverlayLook::Visual {
            kind: ARM.to_owned(),
            states: vec![BlockState {
                name: "probe:look".to_owned(),
                value: StateValue::Choice(look.to_owned()),
            }],
        },
    }
}

fn wit_arm(slot: u8, face: wit::Face, states: Vec<(&str, wit::StateValue)>) -> wit::Overlay {
    wit::Overlay {
        slot,
        face,
        look: wit::OverlayLook::Visual(wit::OverlayVisual {
            kind: ARM.to_owned(),
            states: states
                .into_iter()
                .map(|(name, value)| wit::BlockState {
                    name: name.to_owned(),
                    value,
                })
                .collect(),
        }),
    }
}

fn wit_item(slot: u8, id: &str, metadata: u16, size: f32, x: f32, y: f32) -> wit::Overlay {
    wit::Overlay {
        slot,
        face: wit::Face::South,
        look: wit::OverlayLook::Item(wit::OverlayItem {
            item: wit::ItemKey {
                id: id.to_owned(),
                metadata,
            },
            size,
            x,
            y,
        }),
    }
}

/// A staged list reads back in slot order, restaging replaces its one op, and the block keeps
/// its states and data.
#[test]
fn set_overlays_stages_one_op_that_reads_back_in_slot_order() {
    let mut res = res(vec![arm(0, Face::Up, "plate")]);
    let lit = || ("probe:lit", wit::StateValue::Bool(true));
    let list = vec![
        wit_item(7, STONE, 0, 6.4, 0.0, 0.8),
        wit_arm(2, wit::Face::West, vec![lit()]),
    ];
    assert_eq!(res.set_overlays(ANCHOR, list).unwrap(), Ok(()));
    let read = res.overlays(ANCHOR).unwrap().unwrap();
    assert_eq!(read.iter().map(|o| o.slot).collect::<Vec<_>>(), [2, 7]);
    assert_eq!(res.ops.len(), 1);

    let again = vec![wit_item(3, CELL, 0, 16.0, -8.0, 8.0)];
    assert_eq!(res.set_overlays(ANCHOR, again).unwrap(), Ok(()));
    assert_eq!(res.ops.len(), 1);
    let Op::SetOverlays { pos, overlays } = &res.ops[0] else {
        panic!("expected one set_overlays op, got {:?}", res.ops);
    };
    assert_eq!(*pos, ANCHOR);
    assert_eq!(overlays.iter().map(|o| o.slot).collect::<Vec<_>>(), [3]);

    assert_eq!(res.set_overlays(ANCHOR, Vec::new()).unwrap(), Ok(()));
    assert!(res.overlays(ANCHOR).unwrap().unwrap().is_empty());
    assert_eq!(res.ops.len(), 1);
}

/// Every refusal of the WIT's table, each staging nothing.
#[test]
fn set_overlays_refuses_each_case() {
    let plate = || vec![wit_arm(0, wit::Face::North, Vec::new())];
    let far = BlockPos { x: 40, ..ANCHOR };
    let high = BlockPos { y: 400, ..ANCHOR };
    let thirteen = (0..=MAX_CELL_OVERLAYS as u8)
        .map(|slot| wit_arm(slot, wit::Face::North, Vec::new()))
        .collect::<Vec<_>>();
    let twelve_slot = vec![wit_arm(
        MAX_CELL_OVERLAYS as u8,
        wit::Face::North,
        Vec::new(),
    )];
    let twice = vec![
        wit_arm(1, wit::Face::North, Vec::new()),
        wit_arm(1, wit::Face::South, Vec::new()),
    ];
    let unknown_kind = vec![wit::Overlay {
        slot: 0,
        face: wit::Face::Up,
        look: wit::OverlayLook::Visual(wit::OverlayVisual {
            kind: "probe:counter".to_owned(),
            states: Vec::new(),
        }),
    }];
    let undeclared = vec![wit_arm(
        0,
        wit::Face::Up,
        vec![("probe:speed", wit::StateValue::Bool(true))],
    )];
    let bad_value = vec![wit_arm(
        0,
        wit::Face::Up,
        vec![("probe:look", wit::StateValue::Choice("arm".to_owned()))],
    )];
    let repeated = vec![wit_arm(
        0,
        wit::Face::Up,
        vec![
            ("probe:lit", wit::StateValue::Bool(true)),
            ("probe:lit", wit::StateValue::Bool(false)),
        ],
    )];
    let cases: Vec<(BlockPos, Vec<wit::Overlay>, WorldError)> = vec![
        (far, plate(), WorldError::Denied),
        (high, plate(), WorldError::OutOfBounds),
        (UP, plate(), WorldError::NotOwned),
        (EAST, plate(), WorldError::UnsupportedState),
        (ANCHOR, thirteen, WorldError::TooLarge),
        (ANCHOR, twelve_slot, WorldError::UnsupportedState),
        (ANCHOR, twice, WorldError::UnsupportedState),
        (ANCHOR, unknown_kind, WorldError::UnknownBlock),
        (ANCHOR, undeclared, WorldError::UnsupportedState),
        (ANCHOR, bad_value, WorldError::UnsupportedState),
        (ANCHOR, repeated, WorldError::UnsupportedState),
        (
            ANCHOR,
            vec![wit_item(0, "minecraft:diamond", 0, 4.0, 0.0, 0.0)],
            WorldError::UnknownBlock,
        ),
        (
            ANCHOR,
            vec![wit_item(0, STONE, 1, 4.0, 0.0, 0.0)],
            WorldError::UnknownBlock,
        ),
        (
            ANCHOR,
            vec![wit_item(0, CELL, 3, 4.0, 0.0, 0.0)],
            WorldError::UnknownBlock,
        ),
        (
            ANCHOR,
            vec![wit_item(0, STONE, 0, 0.0, 0.0, 0.0)],
            WorldError::UnsupportedState,
        ),
        (
            ANCHOR,
            vec![wit_item(0, STONE, 0, 16.5, 0.0, 0.0)],
            WorldError::UnsupportedState,
        ),
        (
            ANCHOR,
            vec![wit_item(0, STONE, 0, f32::NAN, 0.0, 0.0)],
            WorldError::UnsupportedState,
        ),
        (
            ANCHOR,
            vec![wit_item(0, STONE, 0, 4.0, 8.5, 0.0)],
            WorldError::UnsupportedState,
        ),
        (
            ANCHOR,
            vec![wit_item(0, STONE, 0, 4.0, 0.0, f32::INFINITY)],
            WorldError::UnsupportedState,
        ),
    ];
    for (pos, overlays, error) in cases {
        let mut res = res(vec![arm(0, Face::Up, "plate")]);
        assert_eq!(
            res.set_overlays(pos, overlays.clone()).unwrap(),
            Err(error),
            "{pos:?} {overlays:?}"
        );
        assert!(res.ops.is_empty(), "{pos:?} staged {:?}", res.ops);
        assert_eq!(res.overlays(ANCHOR).unwrap().unwrap().len(), 1);
    }
}

/// `set-overlays` is one staged op: past the op budget it traps.
#[test]
fn set_overlays_counts_against_the_op_budget() {
    let mut res = res(Vec::new());
    for i in 0..crate::limits::MAX_STAGED_OPS {
        let text = format!("{i}");
        res.ops.push(Op::Tell {
            player: ACTOR.to_owned(),
            text,
        });
    }
    let staged = res.set_overlays(ANCHOR, vec![wit_arm(0, wit::Face::Up, Vec::new())]);
    assert!(limited(staged));
}

/// Replacing the host drops its overlays and the overlays staged for it.
#[test]
fn set_block_clears_the_overlays_of_the_host_it_replaces() {
    let mut res = res(vec![arm(0, Face::Up, "plate")]);
    let staged = res.set_overlays(ANCHOR, vec![wit_arm(4, wit::Face::Up, Vec::new())]);
    assert_eq!(staged.unwrap(), Ok(()));
    assert_eq!(res.set_block(ANCHOR, BUS.to_owned()).unwrap(), Ok(()));
    assert!(res.overlays(ANCHOR).unwrap().unwrap().is_empty());
    assert!(
        res.ops
            .iter()
            .all(|op| !matches!(op, Op::SetOverlays { .. })),
        "{:?}",
        res.ops
    );
}

/// A plate authored against north picks on each face as the table turns it, and nowhere else.
#[test]
fn a_click_picks_a_plate_turned_to_each_face() {
    let faces = [
        (Face::Down, pixel(8.0, 1.0, 8.0)),
        (Face::Up, pixel(8.0, 15.0, 8.0)),
        (Face::North, pixel(8.0, 8.0, 1.0)),
        (Face::South, pixel(8.0, 8.0, 15.0)),
        (Face::West, pixel(1.0, 8.0, 8.0)),
        (Face::East, pixel(15.0, 8.0, 8.0)),
    ];
    for (face, inside) in faces {
        for (other, hit) in faces {
            let picked = target(vec![arm(5, face, "plate")], hit);
            let expected = (other == face).then_some(5);
            assert_eq!(picked, expected, "plate on {face:?} clicked at {hit:?}");
        }
        assert_eq!(target(vec![arm(5, face, "plate")], inside), Some(5));
    }
}

/// Overlapping overlays pick in slot order, whatever order the cell lists them.
#[test]
fn slot_order_breaks_ties() {
    let hit = pixel(8.0, 8.0, 1.0);
    let overlays = vec![arm(9, Face::North, "plate"), arm(3, Face::North, "plate")];
    let mut sorted = overlays.clone();
    sorted.sort_by_key(|overlay| overlay.slot);
    assert_eq!(target(sorted, hit), Some(3));
}

/// A box grown by the pick margin still picks; past it nothing does.
#[test]
fn the_pick_margin_grows_each_box() {
    let plate = || vec![arm(0, Face::North, "plate")];
    assert_eq!(target(plate(), pixel(8.0, 8.0, 2.03)), Some(0));
    assert_eq!(target(plate(), pixel(8.0, 8.0, 2.04)), None);
}

/// A permutation's box and rotation pick, turned before the face's turn; items and types
/// without a box never pick.
#[test]
fn permutations_rotate_the_box_and_some_overlays_never_pick() {
    // The stub's box x 0..4, y 6..10, z 0..6 turns a quarter about z to x 6..10, y 0..4.
    let stub = || vec![arm(1, Face::North, "stub")];
    assert_eq!(target(stub(), pixel(8.0, 2.0, 3.0)), Some(1));
    assert_eq!(target(stub(), pixel(2.0, 8.0, 3.0)), None);
    let bare = Overlay {
        slot: 0,
        face: Face::North,
        look: OverlayLook::Visual {
            kind: BARE.to_owned(),
            states: Vec::new(),
        },
    };
    let item = Overlay {
        slot: 1,
        face: Face::North,
        look: OverlayLook::Item {
            id: STONE.to_owned(),
            metadata: 0,
            size: 16.0,
            x: 0.0,
            y: 0.0,
        },
    };
    assert_eq!(target(vec![bare, item], pixel(8.0, 8.0, 0.0)), None);
}

/// A click outside the cell, or overlays on a cell that is not an overlay host of its own,
/// is malformed.
#[test]
fn malformed_overlay_requests_are_refused() {
    let own = Arc::new(catalog());
    let outside = request(Vec::new(), Some(pixel(8.0, 16.5, 8.0)));
    assert!(prepare(&own, &outside).is_err());
    let mut foreign = request(Vec::new(), None);
    if let Request::Callback { snapshot, .. } = &mut foreign {
        snapshot[1].overlays = vec![arm(0, Face::Up, "plate")];
    }
    assert!(prepare(&own, &foreign).is_err());
}
