//! The core's encodings of a Java Edition server's speed snapshots and explosion pushes.

use std::sync::Arc;

use protocol::{ActorAttribute, ActorAttributeModifier};

fn attribute(current: f32, modifiers: &[(&str, &str, f32, i32)]) -> ActorAttribute {
    ActorAttribute {
        name: Arc::from("cinnabar:test"),
        min: 0.0,
        max: f32::MAX,
        current,
        default: None,
        modifiers: modifiers
            .iter()
            .map(|(id, name, amount, operation)| ActorAttributeModifier {
                id: Arc::from(*id),
                name: Arc::from(*name),
                amount: *amount,
                operation: *operation,
                operand: 0,
                serializable: false,
            })
            .collect(),
    }
}

fn bits(value: f64) -> String {
    format!("{:x}", value.to_bits())
}

/// A speed snapshot keeps its number, its exact base and every modifier's exact amount, id and
/// operation.
#[test]
fn a_speed_snapshot_decodes_exactly() {
    let speed = bits(0.400_000_005_960_464_5);
    let snapshot = super::java_speed_snapshot(&attribute(
        3.0,
        &[
            ("base", &bits(0.1), 0.1, 0),
            ("91aeaa56-376b-4498-935b-2f7f68070635", &speed, 0.4, 2),
        ],
    ))
    .expect("a snapshot");
    assert_eq!(snapshot.sequence, 3);
    assert_eq!(snapshot.base, 0.1);
    assert_eq!(snapshot.modifiers().len(), 1);
    assert_eq!(
        snapshot.modifiers()[0],
        sim::JavaSpeedModifier {
            id: 0x91aeaa56_376b_4498_935b_2f7f68070635,
            amount: 0.400_000_005_960_464_5,
            operation: 2,
        }
    );
    assert_eq!(
        super::java_speed_snapshot(&attribute(1.0, &[("base", "not bits", 0.1, 0)])),
        None,
        "a snapshot that does not decode is left out"
    );
}

/// Recent pushes decode in arrival order with their three axes.
#[test]
fn recent_pushes_decode_in_order() {
    let pushes = super::java_pushes(&attribute(
        5.0,
        &[
            ("5:x", "x", 0.0, 0),
            ("5:y", "y", 0.5, 0),
            ("5:z", "z", -1.0, 0),
            ("4:x", "x", 0.25, 0),
            ("4:y", "y", 0.0, 0),
            ("4:z", "z", 0.0, 0),
        ],
    ));
    assert_eq!(
        pushes.pushes(),
        [
            sim::JavaPush {
                sequence: 4,
                motion: [0.25, 0.0, 0.0],
            },
            sim::JavaPush {
                sequence: 5,
                motion: [0.0, 0.5, -1.0],
            },
        ]
    );
}

/// The translator's own encodings, from the fixture its Go test writes, decode to the values
/// it encoded.
#[test]
fn the_translators_encodings_decode_exactly() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../../fixtures/java_attributes.json"))
            .expect("the fixture is JSON");
    let decode_attribute = |value: &serde_json::Value| ActorAttribute {
        name: Arc::from(value["name"].as_str().expect("name")),
        min: 0.0,
        max: f32::MAX,
        current: value["current"].as_f64().expect("current") as f32,
        default: None,
        modifiers: value["modifiers"]
            .as_array()
            .expect("modifiers")
            .iter()
            .map(|modifier| ActorAttributeModifier {
                id: Arc::from(modifier["id"].as_str().expect("id")),
                name: Arc::from(modifier["name"].as_str().expect("modifier name")),
                amount: modifier["amount"].as_f64().expect("amount") as f32,
                operation: modifier["operation"].as_i64().expect("operation") as i32,
                operand: 0,
                serializable: false,
            })
            .collect(),
    };
    let bits = |value: &serde_json::Value| {
        f64::from_bits(u64::from_str_radix(value.as_str().expect("bits"), 16).expect("hex bits"))
    };
    let expected = &fixture["expected"];

    let snapshot = super::java_speed_snapshot(&decode_attribute(&fixture["speed"]))
        .expect("the translator's snapshot decodes");
    assert_eq!(
        snapshot.base.to_bits(),
        bits(&expected["speed_base"]).to_bits()
    );
    assert_eq!(snapshot.modifiers().len(), 1);
    assert_eq!(
        snapshot.modifiers()[0].id,
        u128::from_str_radix(expected["speed_modifier_id"].as_str().expect("id"), 16)
            .expect("hex id")
    );
    assert_eq!(
        snapshot.modifiers()[0].amount.to_bits(),
        bits(&expected["speed_modifier_bits"]).to_bits()
    );
    assert_eq!(snapshot.modifiers()[0].operation, 2);

    let pushes = super::java_pushes(&decode_attribute(&fixture["pushes"]));
    let want: Vec<[f64; 3]> = expected["pushes"]
        .as_array()
        .expect("pushes")
        .iter()
        .map(|push| std::array::from_fn(|axis| bits(&push[axis])))
        .collect();
    let got: Vec<[f64; 3]> = pushes.pushes().iter().map(|push| push.motion).collect();
    assert_eq!(got, want);
}
