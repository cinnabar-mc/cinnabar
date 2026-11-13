//! A Java Edition server reads positions in double precision, so on a Java session the
//! movement input carries what its single-precision position loses.
//!
//! `java_positions.tsv` lists positions with the packet fields that carry them. This side
//! writes the rows; the core's translator reads the same rows back to the same doubles.

use bedrock_protocol_1_26_60::packets;
use protocol::{PLAYER_NETWORK_OFFSET, PlayerAuthInputSnapshot, player_auth_input};

const FIXTURE: &str = "fixtures/java_positions.tsv";
const HEADER: &str = "# Feet x y z as double bits, then the input's position, delta and camera \
                      orientation x y z as single bits, all hexadecimal.";

/// Positions at rest, mid-jump, against blocks, below zero and far out, then a seeded spread.
fn positions() -> Vec<[f64; 3]> {
    let mut positions = vec![
        [0.5, 4.0, 0.5],
        [-587.5, 4.0, 262.308_105_468_75],
        [
            8.123_456_789_012_345,
            64.419_999_986_886_98,
            -3_000_000.000_123,
        ],
        [29_999_984.7, 255.0, -29_999_984.3],
        [0.300_000_000_1, 0.0, -0.699_999_99],
        [1.0e-3, 1.5e-2, -1.0e-3],
        [123_456.789, 4.753_200_024_967_194, 9.876_543_21e-5],
        [-0.5, -12.250_000_01, 77.7],
        [63.999_999_999_99, 63.999_999_999_99, 64.000_000_000_01],
    ];
    let mut seed = 0x9E37_79B9_7F4A_7C15_u64;
    let mut next = |scale: f64| {
        seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((seed >> 11) as f64 / (1_u64 << 53) as f64 - 0.5) * scale
    };
    for scale in [2.0, 200.0, 20_000.0, 2.0e6, 6.0e7] {
        for _ in 0..8 {
            positions.push([next(scale), next(400.0) + 64.0, next(scale)]);
        }
    }
    positions
}

fn hex32(values: [f32; 3]) -> String {
    values
        .map(|value| format!("{:08x}", value.to_bits()))
        .join("\t")
}

#[test]
fn java_positions_cross_the_wire_exactly() {
    let mut rows = vec![HEADER.to_owned()];
    for feet in positions() {
        let wire = [
            feet[0] as f32,
            feet[1] as f32 + PLAYER_NETWORK_OFFSET,
            feet[2] as f32,
        ];
        let packet = player_auth_input(PlayerAuthInputSnapshot {
            position: wire,
            delta: [0.1, -0.078_4, 0.2],
            camera_orientation: [0.0, 0.0, 1.0],
            java_position: Some(feet),
            ..super::player_auth_input::snapshot()
        })
        .expect("a valid input");
        let packets::Packet::PlayerAuthInput(input) = &*packet.data else {
            panic!("the builder returns a PlayerAuthInput");
        };
        let (position, first, second) = (
            input.position.to_array(),
            input.pos_delta.to_array(),
            input.camera_orientation.to_array(),
        );
        assert_eq!(position, wire);
        // The translator's reading of the packet.
        let base = [
            f64::from(position[0]),
            f64::from(position[1]) - f64::from(PLAYER_NETWORK_OFFSET),
            f64::from(position[2]),
        ];
        let restored: [f64; 3] = std::array::from_fn(|axis| {
            base[axis] + (f64::from(first[axis]) + f64::from(second[axis]))
        });
        assert_eq!(
            restored.map(f64::to_bits),
            feet.map(f64::to_bits),
            "{feet:?} restored as {restored:?}"
        );
        rows.push(format!(
            "{}\t{}\t{}\t{}",
            feet.map(|value| format!("{:016x}", value.to_bits()))
                .join("\t"),
            hex32(position),
            hex32(first),
            hex32(second)
        ));
    }
    let text = rows.join("\n") + "\n";
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURE);
    if std::env::var_os("CINNABAR_UPDATE_FIXTURES").is_some() {
        std::fs::write(&path, &text).expect("write the fixture");
    }
    let listed = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("missing fixture: {}: {error}", path.display()));
    assert_eq!(
        listed, text,
        "{FIXTURE} is stale; rerun with CINNABAR_UPDATE_FIXTURES=1"
    );
}

/// Without a Java position the delta and camera orientation are the player's own.
#[test]
fn a_bedrock_input_keeps_its_delta_and_camera_orientation() {
    let packet = player_auth_input(PlayerAuthInputSnapshot {
        delta: [0.1, -0.078_4, 0.2],
        camera_orientation: [0.0, 0.6, 0.8],
        ..super::player_auth_input::snapshot()
    })
    .expect("a valid input");
    let packets::Packet::PlayerAuthInput(input) = &*packet.data else {
        panic!("the builder returns a PlayerAuthInput");
    };
    assert_eq!(input.pos_delta.to_array(), [0.1, -0.078_4, 0.2]);
    assert_eq!(input.camera_orientation.to_array(), [0.0, 0.6, 0.8]);
}
