//! Server WIT 0.10's overlay registration: hosts and overlay types at the limits, and each
//! refusal.

use super::super::overlays;
use super::*;
use crate::limits::MAX_OVERLAY_TYPES;

/// An overlay type `name`: one bool state, the cable's geometry drawn with `counter.png`, a
/// selection box, and a permutation that turns it while the state holds.
fn arm(name: &str) -> wit::OverlayType {
    let mut turned = permutation(vec![vec![is("lit", true)]]);
    turned.rotation = Some(turns(0, 0, 1));
    wit::OverlayType {
        name: name.to_owned(),
        states: vec![bool_state("lit")],
        visual: wit::Visual {
            geometry: Some("cable.geo.json".to_owned()),
            materials: vec![
                material("*", "counter.png"),
                material("base", "counter.png"),
            ],
            bones: Vec::new(),
            collision: None,
            selection: Some(pixel_box(6.0, 10.0)),
            rotation: None,
        },
        permutations: vec![turned],
    }
}

fn registration(hosts: &[&str], types: Vec<wit::OverlayType>) -> wit::OverlayRegistration {
    wit::OverlayRegistration {
        hosts: hosts.iter().map(|&host| host.to_owned()).collect(),
        types,
    }
}

/// Each row changes a valid registration for an Experience whose one block is the cable. Blocks
/// and overlay types share one geometry index, so another file under an identifier the cable
/// uses is refused.
#[test]
fn overlay_rules_accept_limits_and_refuse_violations() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = rule_assets(dir.path());
    let types = |count: usize| (0..count).map(|i| arm(&format!("arm_{i}"))).collect();
    let with = |edit: fn(&mut wit::OverlayType)| {
        let mut overlay = arm("arm");
        edit(&mut overlay);
        vec![overlay]
    };
    let cases: Vec<(&str, wit::OverlayRegistration, Option<&str>)> = vec![
        (
            "the baseline",
            registration(&["probe:cable"], types(1)),
            None,
        ),
        ("no overlays", registration(&[], Vec::new()), None),
        (
            "MAX_OVERLAY_TYPES types",
            registration(&["probe:cable"], types(MAX_OVERLAY_TYPES)),
            None,
        ),
        (
            "one type too many",
            registration(&["probe:cable"], types(MAX_OVERLAY_TYPES + 1)),
            Some("the limit is 128"),
        ),
        (
            "a host that is not a block",
            registration(&["probe:nothing"], types(1)),
            Some("is not one of the Experience's blocks"),
        ),
        (
            "a host twice",
            registration(&["probe:cable", "probe:cable"], types(1)),
            Some("listed twice"),
        ),
        (
            "an invalid name",
            registration(&[], vec![arm("Arm")]),
            Some("invalid name"),
        ),
        (
            "a name too long",
            registration(&[], vec![arm(&"a".repeat(MAX_BLOCK_NAME_BYTES + 1))]),
            Some("invalid name"),
        ),
        (
            "the name of a block",
            registration(&[], vec![arm("cable")]),
            Some("the id of a block"),
        ),
        (
            "a type twice",
            registration(&[], vec![arm("arm"), arm("arm")]),
            Some("declared twice"),
        ),
        (
            "a state outside the namespace",
            registration(&[], with(|o| o.states[0].name = "other:lit".to_owned())),
            Some("is not probe:<name>"),
        ),
        (
            "a condition on a state it lacks",
            registration(&[], with(|o| o.states.clear())),
            Some("which the block does not have"),
        ),
        (
            "an identifier the cable's file has",
            registration(
                &[],
                with(|o| o.visual.geometry = Some("alt.geo.json".to_owned())),
            ),
            Some("geometry.probe.cable"),
        ),
    ];
    let mut failures = Vec::new();
    for (case, registration, refusal) in cases {
        let mut assets = assets(dir.path(), &manifest);
        let blocks = validate_blocks(&mut assets, vec![cable()]).unwrap();
        let outcome = overlays::validate(&mut assets, &blocks, registration)
            .map_err(|error| format!("{error:#}"));
        match (outcome, refusal) {
            (Ok(_), None) => {}
            (Ok(_), Some(cause)) => failures.push(format!("{case}: accepted, not {cause:?}")),
            (Err(error), None) => failures.push(format!("{case}: refused: {error}")),
            (Err(error), Some(cause)) if !error.contains(cause) => {
                failures.push(format!("{case}: {error:?} lacks {cause:?}"));
            }
            (Err(_), Some(_)) => {}
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

/// A valid type keeps its id, states and checked look, with absolute asset paths.
#[test]
fn validated_overlay_types_carry_their_id_and_look() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = rule_assets(dir.path());
    let mut assets = assets(dir.path(), &manifest);
    let blocks = validate_blocks(&mut assets, vec![cable()]).unwrap();
    let checked = overlays::validate(
        &mut assets,
        &blocks,
        registration(&["probe:cable"], vec![arm("arm")]),
    )
    .unwrap();
    assert_eq!(checked.hosts, ["probe:cable"]);
    let [overlay] = checked.types.as_slice() else {
        panic!("one type, got {:?}", checked.types);
    };
    assert_eq!(overlay.id, "probe:arm");
    assert_eq!(
        overlay.states,
        [StateDef {
            name: "probe:lit".to_owned(),
            values: StateValues::Bool,
        }]
    );
    let geometry = dir.path().join(ASSETS_DIR).join("cable.geo.json");
    assert_eq!(
        overlay.visual.geometry.as_deref(),
        Some(geometry.to_str().unwrap())
    );
    assert_eq!(overlay.permutations.len(), 1);
}
