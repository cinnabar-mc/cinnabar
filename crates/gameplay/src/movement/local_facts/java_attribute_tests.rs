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
