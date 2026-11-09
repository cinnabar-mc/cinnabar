//! Server WIT 0.10's cell overlays end to end on the probe: its overlay registration, and using
//! its bus, which adds a plate on the clicked face or removes the plate the click targets.

use experience_runtime::load::{engine, load};
use experience_runtime::protocol::{
    Call, Face, Op, Outcome, Overlay, OverlayDef, OverlayLook, Pixel, PixelBox, Request, Visual,
};

use crate::common::{self, ACTOR, BUS, callback, interact, outcome, p, probe, tell};

/// The bus at `p(0)`, carrying `overlays`, used on `face` at `hit`.
fn use_bus(overlays: Vec<Overlay>, face: Face, hit: Pixel) -> Request {
    let mut request = callback(
        p(0),
        Call::Interact {
            player: ACTOR.to_owned(),
            pos: p(0),
            face,
            hit: Some(hit),
        },
    );
    if let Request::Callback { snapshot, .. } = &mut request {
        snapshot[0].id = BUS.to_owned();
        snapshot[0].overlays = overlays;
    }
    request
}

fn plate(slot: u8, face: Face) -> Overlay {
    Overlay {
        slot,
        face,
        look: OverlayLook::Visual {
            kind: "probe:plate".to_owned(),
            states: Vec::new(),
        },
    }
}

#[test]
fn the_probe_declares_its_bus_and_plate() {
    let overlays = probe().loaded.overlays.clone().expect("a 0.10 world");
    assert_eq!(overlays.hosts, [BUS]);
    let [OverlayDef { id, visual, .. }] = overlays.types.as_slice() else {
        panic!("one overlay type, got {:?}", overlays.types);
    };
    assert_eq!(id, "probe:plate");
    let Visual {
        geometry,
        selection,
        ..
    } = visual;
    let geometry = geometry.as_deref().expect("the plate has its geometry");
    assert!(
        std::path::Path::new(geometry).is_absolute() && geometry.ends_with("plate.geo.json"),
        "{geometry}"
    );
    let PixelBox { min, max } = selection.expect("the plate's box picks it");
    assert_eq!(
        ([min.x, min.y, min.z], [max.x, max.y, max.z]),
        ([0.0; 3], [16.0, 16.0, 2.0])
    );
}

/// A click on a bus without plates adds one on the clicked face; a click on that plate, as the
/// cell then carries it, targets its slot and removes it.
#[test]
fn using_the_bus_adds_a_plate_then_a_click_on_it_removes_it() {
    let west = Pixel {
        x: 0.0,
        y: 8.0,
        z: 8.0,
    };
    assert_eq!(
        outcome(&use_bus(Vec::new(), Face::West, west)),
        Outcome::Committed {
            ops: vec![
                Op::SetOverlays {
                    pos: p(0),
                    overlays: vec![plate(4, Face::West)],
                },
                tell("plate 4 ok"),
            ]
        }
    );
    let on_plate = Pixel { x: 1.5, ..west };
    assert_eq!(
        outcome(&use_bus(vec![plate(4, Face::West)], Face::West, on_plate)),
        Outcome::Committed {
            ops: vec![
                Op::SetOverlays {
                    pos: p(0),
                    overlays: Vec::new(),
                },
                tell("target 4 ok"),
            ]
        }
    );
}

/// A frozen 0.9 artifact keeps its ABI: it loads without overlays, and its `on-interact` gets
/// no click.
#[test]
fn v0_9_artifact_loads_and_runs_without_overlays() {
    let dir = common::v0_9_dir();
    let (engine, _ticker) = engine().unwrap();
    let loaded = load(&engine, dir.path()).unwrap();
    assert_eq!(loaded.manifest.api, "0.9");
    assert_eq!(loaded.overlays, None);
    assert_eq!(
        experience_runtime::callback::run(&engine, &loaded, &interact(0)),
        Outcome::Committed {
            ops: vec![tell("v0.9")]
        }
    );
}
