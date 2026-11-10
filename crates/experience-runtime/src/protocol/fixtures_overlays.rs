//! Cross-language fixtures for cell overlays: the overlay registration `loaded` carries, a host
//! cell's overlays and a click position in an interaction, and the `set_overlays` op.
use super::*;

/// Covers both overlay looks, every face, a state left to its first value and a click.
pub(super) fn fixtures() -> Vec<(&'static str, String)> {
    let host = BlockPos { x: 3, y: 70, z: -2 };
    let asset = |file: &str| format!("/srv/experiences/benergistics/assets/{file}");
    let state = |name: &str, value: StateValue| BlockState {
        name: name.to_owned(),
        value,
    };
    let pixel = |x: f32, y: f32, z: f32| Pixel { x, y, z };
    let arm = |slot: u8, face: Face| Overlay {
        slot,
        face,
        look: OverlayLook::Visual {
            kind: "benergistics:glass_cable_arm".to_owned(),
            states: vec![state("benergistics:dyed", StateValue::Bool(true))],
        },
    };
    let monitor = Overlay {
        slot: 11,
        face: Face::South,
        look: OverlayLook::Item {
            id: "benergistics:certus_quartz_crystal".to_owned(),
            metadata: 0,
            size: 6.4,
            x: -0.25,
            y: 0.8,
        },
    };
    let overlays: Vec<Overlay> = [Face::Down, Face::Up, Face::North, Face::West, Face::East]
        .into_iter()
        .enumerate()
        .map(|(slot, face)| arm(slot as u8, face))
        .chain([monitor])
        .collect();
    let registration = OverlayRegistration {
        hosts: vec!["benergistics:cable_bus".to_owned()],
        types: vec![OverlayDef {
            id: "benergistics:glass_cable_arm".to_owned(),
            states: vec![
                StateDef {
                    name: "benergistics:dyed".to_owned(),
                    values: StateValues::Bool,
                },
                StateDef {
                    name: "benergistics:channels".to_owned(),
                    values: StateValues::Choices(vec!["0".to_owned(), "8".to_owned()]),
                },
            ],
            visual: Visual {
                geometry: Some(asset("glass_cable_arm.geo.json")),
                materials: vec![Material {
                    instance: "*".to_owned(),
                    path: asset("glass_cable.png"),
                    render_method: RenderMethod::AlphaTest,
                    flipbook: None,
                }],
                bones: Vec::new(),
                collision: None,
                selection: Some(PixelBox {
                    min: pixel(5.5, 5.5, 0.5),
                    max: pixel(10.5, 10.5, 6.5),
                }),
                rotation: None,
            },
            permutations: vec![Permutation {
                when: vec![vec![StateTest {
                    state: state("benergistics:channels", StateValue::Choice("8".to_owned())),
                    equal: true,
                }]],
                geometry: None,
                materials: None,
                bones: None,
                collision: None,
                selection: None,
                rotation: Some(QuarterTurns { x: 0, y: 0, z: 1 }),
            }],
        }],
    };
    let loaded = Response::Loaded {
        entities: Vec::new(),
        simulation: true,
        protocol: PROTOCOL_VERSION,
        id: "benergistics".to_owned(),
        version: "0.1.0".to_owned(),
        blocks: Vec::new(),
        items: Vec::new(),
        focus: true,
        worlds: None,
        overlays: Some(registration),
    };
    let interact = Request::Callback {
        seq: 9,
        info: Info {
            context: None,
            simulation: None,
            world_id: "world".to_owned(),
            dimension_id: "overworld".to_owned(),
            tick: 120,
            event_sequence: 77,
        },
        actor: Some("6f1c3c2e-5b7a-4d3e-9a51-0c8e2f4b7d10".to_owned()),
        world_min_y: -64,
        world_max_y: 319,
        data_budget: 4096,
        snapshot: vec![Cell {
            pos: host,
            loaded: true,
            id: "benergistics:cable_bus".to_owned(),
            owned: true,
            data: None,
            states: Vec::new(),
            overlays: overlays.clone(),
            // The bus is a redstone block, here beside a powered lever.
            power: 15,
        }],
        network: None,
        inventory: None,
        containers: None,
        call: Call::Interact {
            player: "6f1c3c2e-5b7a-4d3e-9a51-0c8e2f4b7d10".to_owned(),
            pos: host,
            face: Face::North,
            hit: Some(pixel(8.25, 7.5, 0.25)),
        },
    };
    let committed = Response::Result {
        seq: 9,
        outcome: Outcome::Committed {
            ops: vec![
                Op::SetOverlays {
                    pos: host,
                    overlays,
                },
                Op::SetOverlays {
                    pos: BlockPos { x: 4, ..host },
                    overlays: Vec::new(),
                },
            ],
        },
    };
    vec![
        ("response_loaded_overlays", pretty(&loaded)),
        ("request_callback_interact_overlay", pretty(&interact)),
        ("response_result_overlays", pretty(&committed)),
    ]
}

/// `message` as pretty JSON with a trailing newline.
fn pretty(message: &impl serde::Serialize) -> String {
    serde_json::to_string_pretty(message).expect("fixtures serialize") + "\n"
}
