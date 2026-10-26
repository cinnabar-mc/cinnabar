use super::*;
use assets::facts::{BlockProperty, StateValue};
use assets::{BlockFace, CompiledBiomeAssets, MODEL_UV_SCALE, NetworkIdMode, RuntimeAssets};

const D: BlockFace = BlockFace::Down;
const U: BlockFace = BlockFace::Up;
const N: BlockFace = BlockFace::North;
const S: BlockFace = BlockFace::South;
const W: BlockFace = BlockFace::West;
const E: BlockFace = BlockFace::East;

/// World faces in `facing_direction` order, the column order of the tables below.
const WORLD: [BlockFace; 6] = [D, U, N, S, W, E];

/// Base-game observer faces for each `minecraft:facing_direction`: the authored slot and UV quarter
/// turns each world face (down, up, north, south, west, east) draws.
const OBSERVER: [[(BlockFace, u8); 6]; 6] = [
    [(N, 0), (S, 0), (U, 0), (D, 0), (E, 2), (W, 1)],
    [(S, 0), (N, 0), (U, 3), (D, 3), (E, 1), (W, 2)],
    [(D, 0), (U, 3), (N, 0), (S, 0), (W, 0), (E, 3)],
    [(D, 3), (U, 0), (S, 0), (N, 0), (W, 3), (E, 0)],
    [(D, 1), (U, 1), (E, 3), (W, 0), (N, 0), (S, 0)],
    [(D, 2), (U, 2), (E, 0), (W, 3), (S, 0), (N, 0)],
];

/// Base-game crafter faces for each `orientation`, in its value order.
const CRAFTER: [(&str, [(BlockFace, u8); 6]); 12] = [
    (
        "down_east",
        [(N, 1), (S, 1), (W, 1), (E, 1), (D, 0), (U, 0)],
    ),
    (
        "down_north",
        [(N, 3), (S, 0), (U, 0), (D, 0), (W, 2), (E, 2)],
    ),
    (
        "down_south",
        [(N, 0), (S, 3), (D, 0), (U, 0), (E, 1), (W, 1)],
    ),
    (
        "down_west",
        [(N, 2), (S, 2), (E, 2), (W, 2), (U, 0), (D, 0)],
    ),
    ("up_east", [(S, 2), (N, 2), (W, 2), (E, 2), (U, 3), (D, 3)]),
    ("up_north", [(S, 0), (N, 3), (D, 3), (U, 3), (W, 1), (E, 1)]),
    ("up_south", [(S, 3), (N, 0), (U, 3), (D, 3), (E, 2), (W, 2)]),
    ("up_west", [(S, 1), (N, 1), (E, 1), (W, 1), (D, 3), (U, 3)]),
    ("west_up", [(D, 1), (U, 1), (E, 0), (W, 0), (N, 0), (S, 0)]),
    ("east_up", [(D, 2), (U, 2), (W, 0), (E, 0), (S, 0), (N, 0)]),
    ("north_up", [(D, 0), (U, 3), (N, 0), (S, 0), (W, 0), (E, 0)]),
    ("south_up", [(D, 3), (U, 0), (S, 0), (N, 0), (E, 0), (W, 0)]),
];

/// Base-game piston body faces for each `facing_direction`; the head (the top slot) points
/// opposite the horizontal facings.
const PISTON: [[(BlockFace, u8); 6]; 6] = [
    [(U, 0), (D, 0), (S, 3), (S, 3), (S, 3), (S, 3)],
    [(D, 0), (U, 0), (S, 0), (S, 0), (S, 0), (S, 0)],
    [(S, 3), (S, 3), (D, 0), (U, 0), (S, 1), (S, 1)],
    [(S, 0), (S, 0), (U, 0), (D, 0), (S, 2), (S, 2)],
    [(S, 1), (S, 1), (S, 1), (S, 1), (D, 0), (U, 0)],
    [(S, 2), (S, 2), (S, 2), (S, 2), (U, 0), (D, 0)],
];

/// Fixture terrain keys with one artwork per array entry; slot keys share no artwork, so
/// each face's material names the slot and variant it drew.
const TERRAIN: [(&str, usize); 18] = [
    ("observer_north", 2),
    ("observer_south", 2),
    ("observer_east", 2),
    ("observer_west", 2),
    ("observer_top", 2),
    ("observer_bottom", 2),
    ("crafter_north", 3),
    ("crafter_south", 3),
    ("crafter_east", 3),
    ("crafter_west", 3),
    ("crafter_top", 3),
    ("crafter_bottom", 3),
    ("dispenser_front_horizontal", 1),
    ("dispenser_front_vertical", 1),
    ("dispenser_side", 1),
    ("dispenser_top", 1),
    ("piston_side", 1),
    ("stone", 1),
];
const PISTON_TERRAIN: [&str; 2] = ["piston_top", "piston_bottom"];

/// Every fixture artwork name, `key` or `key_<variant>` for arrays.
fn artwork() -> Vec<String> {
    TERRAIN
        .iter()
        .flat_map(|&(key, count)| {
            (0..count).map(move |variant| {
                if count == 1 {
                    key.to_owned()
                } else {
                    format!("{key}_{variant}")
                }
            })
        })
        .chain(PISTON_TERRAIN.iter().map(|&key| key.to_owned()))
        .collect()
}

/// The original artwork's red channel identifies its sprite independently of atlas ordering.
fn artwork_tag(name: &str) -> u8 {
    6 + artwork().iter().position(|art| art == name).unwrap() as u8 * 6
}

/// Reads the material's sprite identity through its actual texture page and layer.
fn material_tag(compiled: &assets::CompiledAssets, material: u32) -> u8 {
    let reference = compiled.materials[material as usize].texture;
    let mip = &compiled.texture_pages[reference.page() as usize]
        .texture
        .mips[0];
    mip.rgba8[(reference.layer() * mip.size * mip.size * 4) as usize]
}

/// Compiles every registered state of the mapped-face blocks against the fixture pack.
fn compile() -> (assets::CompiledAssets, Vec<RegistryRecord>) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let target: serde_json::Value =
        serde_json::from_slice(include_bytes!("../../../../../assets/bedrock-target.json"))
            .unwrap();
    let protocol = target["wire_protocol"].as_u64().unwrap() as u32;
    let registry_bytes =
        fs::read(root.join(target["artifacts"]["block_registry"].as_str().unwrap())).unwrap();
    let registry = assets::read_registry_for_protocol(&registry_bytes, protocol).unwrap();
    let lights = assets::read_light_registry_for_protocol(
        &fs::read(root.join(target["artifacts"]["light_registry"].as_str().unwrap())).unwrap(),
        &registry_bytes,
        registry.len(),
        protocol,
    )
    .unwrap();
    let records = registry
        .into_vec()
        .into_iter()
        .filter(|record| {
            matches!(
                record.name.as_ref(),
                "minecraft:air"
                    | "minecraft:stone"
                    | "minecraft:observer"
                    | "minecraft:crafter"
                    | "minecraft:dispenser"
                    | "minecraft:dropper"
                    | "minecraft:piston"
                    | "minecraft:sticky_piston"
            )
        })
        .collect::<Vec<_>>();
    let span = records
        .iter()
        .map(|r| r.sequential_id as usize + 1)
        .max()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let faces = |prefix: &str| {
        format!(
            r#"{{"down":"{prefix}_bottom","up":"{prefix}_top","north":"{prefix}_north","south":"{prefix}_south","west":"{prefix}_west","east":"{prefix}_east"}}"#
        )
    };
    write(
        directory.path().join("blocks.json"),
        format!(
            r#"{{
            "stone":{{"textures":"stone"}},
            "observer":{{"textures":{}}},
            "crafter":{{"textures":{}}},
            "dispenser":{{"textures":{{"down":"dispenser_top","up":"dispenser_top","north":"dispenser_side","south":"dispenser_front_horizontal","west":"dispenser_side","east":"dispenser_front_vertical"}}}},
            "dropper":{{"textures":{{"down":"dispenser_top","up":"dispenser_top","north":"dispenser_side","south":"dispenser_front_horizontal","west":"dispenser_side","east":"dispenser_front_vertical"}}}},
            "piston":{{"textures":{{"down":"piston_bottom","side":"piston_side","up":"piston_top"}}}},
            "sticky_piston":{{"textures":{{"down":"piston_bottom","side":"piston_side","up":"piston_top"}}}}
        }}"#,
            faces("observer"),
            faces("crafter")
        ),
    );
    let texture_data = TERRAIN
        .iter()
        .map(|&(key, count)| {
            let paths = (0..count)
                .map(|variant| {
                    if count == 1 {
                        format!(r#""textures/blocks/{key}""#)
                    } else {
                        format!(r#""textures/blocks/{key}_{variant}""#)
                    }
                })
                .collect::<Vec<_>>()
                .join(",");
            format!(r#""{key}":{{"textures":[{paths}]}}"#)
        })
        .chain(
            PISTON_TERRAIN
                .iter()
                .map(|key| format!(r#""{key}":{{"textures":"textures/blocks/{key}"}}"#)),
        )
        .collect::<Vec<_>>()
        .join(",");
    write(
        directory.path().join("textures/terrain_texture.json"),
        format!(r#"{{"texture_data":{{{texture_data}}}}}"#),
    );
    write(
        directory.path().join("textures/flipbook_textures.json"),
        "[]",
    );
    for name in artwork() {
        let pixels = (0..TILE_SIZE * TILE_SIZE)
            .flat_map(|pixel| {
                [
                    artwork_tag(&name),
                    (pixel % TILE_SIZE) as u8 * 12,
                    (pixel / TILE_SIZE) as u8 * 12,
                    255,
                ]
            })
            .collect::<Vec<_>>();
        write_png(
            directory.path().join(format!("textures/blocks/{name}.png")),
            TILE_SIZE,
            TILE_SIZE,
            &pixels,
        );
    }
    let (compiled, _) = super::super::compile_pack_inner(
        directory.path(),
        &records,
        &lights[..span],
        CompiledBiomeAssets::diagnostic(),
        protocol,
    )
    .unwrap();
    (compiled, records)
}

/// The model quad a state draws for one world face.
fn quad(runtime: &RuntimeAssets, record: &RegistryRecord, face: BlockFace) -> assets::ModelQuad {
    let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
    let template = runtime.model_templates()[visual
        .model_template()
        .unwrap_or_else(|| panic!("{} {} draws a model", record.name, record.canonical_state))
        as usize];
    let quad = runtime.model_quads()[template.quad_start as usize..]
        [..template.quad_count as usize][face as usize];
    assert_eq!(quad.material, visual.face(face).material_id());
    quad
}

/// Quarter turns (1 and 2 opposite, 3 a half) carrying `reference` UVs onto `turned`.
fn quarter_turns(reference: &assets::ModelQuad, turned: &assets::ModelQuad) -> Option<u8> {
    let scale = MODEL_UV_SCALE;
    (0..4).find(|&turn| {
        reference.uvs.iter().zip(turned.uvs).all(|(&[u, v], uv)| {
            uv == match turn {
                1 => [v, scale - u],
                2 => [scale - v, u],
                3 => [scale - u, scale - v],
                _ => [u, v],
            }
        })
    })
}

fn int(record: &RegistryRecord, key: BlockProperty) -> usize {
    match record.property(key) {
        Some(StateValue::Int(value)) => *value as usize,
        Some(StateValue::Bool(value)) => usize::from(*value),
        other => panic!("{} {key:?}: {other:?}", record.canonical_state),
    }
}

/// Checks one state's six faces against its expected slots, turns and artwork names.
fn assert_faces(
    compiled: &assets::CompiledAssets,
    runtime: &RuntimeAssets,
    reference: &RegistryRecord,
    record: &RegistryRecord,
    expected: [(BlockFace, u8); 6],
    artwork: impl Fn(BlockFace) -> String,
) {
    for (world, (slot, turn)) in WORLD.into_iter().zip(expected) {
        let drawn = quad(runtime, record, world);
        assert_eq!(
            material_tag(compiled, drawn.material),
            artwork_tag(&artwork(slot)),
            "{} {} {world:?} draws the {slot:?} slot",
            record.name,
            record.canonical_state
        );
        assert_eq!(
            quarter_turns(&quad(runtime, reference, world), &drawn),
            Some(turn),
            "{} {} {world:?} turns its artwork",
            record.name,
            record.canonical_state
        );
    }
}

/// Every registered state of one block.
fn named<'a>(
    records: &'a [RegistryRecord],
    name: &'a str,
) -> impl Iterator<Item = &'a RegistryRecord> {
    records
        .iter()
        .filter(move |record| record.name.as_ref() == name)
}

fn slot_name(face: BlockFace) -> &'static str {
    match face {
        D => "bottom",
        U => "top",
        N => "north",
        S => "south",
        W => "west",
        E => "east",
    }
}

#[test]
fn mapped_face_blocks_turn_their_authored_faces_with_their_state() {
    let (compiled, records) = compile();
    let runtime = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    // An upward piston body draws every face unturned, the reference for each face's UVs.
    let reference = named(&records, "minecraft:piston")
        .find(|r| int(r, BlockProperty::FacingDirection) == 1)
        .unwrap();

    let mut seen = 0;
    for record in named(&records, "minecraft:observer") {
        let powered = int(record, BlockProperty::PoweredBit);
        let StateValue::String(facing) = record
            .property(BlockProperty::MinecraftFacingDirection)
            .unwrap()
        else {
            panic!("{}", record.canonical_state);
        };
        let facing = ["down", "up", "north", "south", "west", "east"]
            .iter()
            .position(|name| *name == facing.as_ref())
            .unwrap();
        let expected = OBSERVER[facing];
        assert_faces(&compiled, &runtime, reference, record, expected, |slot| {
            // Only the back has distinct powered artwork in the base game; every slot here
            // keeps its own array, so the selected entry is visible on all six faces.
            format!("observer_{}_{powered}", slot_name(slot))
        });
        seen += 1;
    }
    assert_eq!(seen, 12, "six facings, powered and unpowered");

    let mut seen = 0;
    for record in named(&records, "minecraft:crafter") {
        let StateValue::String(orientation) = record.property(BlockProperty::Orientation).unwrap()
        else {
            panic!("{}", record.canonical_state);
        };
        let (_, expected) = CRAFTER
            .iter()
            .find(|(name, _)| *name == orientation.as_ref())
            .unwrap();
        let variant = if int(record, BlockProperty::Crafting) == 1 {
            2
        } else {
            int(record, BlockProperty::TriggeredBit)
        };
        assert_faces(&compiled, &runtime, reference, record, *expected, |slot| {
            format!("crafter_{}_{variant}", slot_name(slot))
        });
        seen += 1;
    }
    assert_eq!(
        seen, 48,
        "twelve orientations in every triggered and crafting state"
    );

    for record in named(&records, "minecraft:dispenser").chain(named(&records, "minecraft:dropper"))
    {
        let facing = int(record, BlockProperty::FacingDirection);
        let vertical = facing < 2;
        let expected = WORLD.map(|world| {
            let slot = match (world == WORLD[facing], vertical) {
                (true, true) => E,
                (true, false) => S,
                (false, true) => U,
                (false, false) if !world.is_horizontal() => U,
                (false, false) => N,
            };
            (slot, 0)
        });
        assert_faces(&compiled, &runtime, reference, record, expected, |slot| {
            match slot {
                E => "dispenser_front_vertical",
                S => "dispenser_front_horizontal",
                U => "dispenser_top",
                _ => "dispenser_side",
            }
            .to_owned()
        });
    }

    for record in
        named(&records, "minecraft:piston").chain(named(&records, "minecraft:sticky_piston"))
    {
        let expected = PISTON[int(record, BlockProperty::FacingDirection)];
        assert_faces(&compiled, &runtime, reference, record, expected, |slot| {
            match slot {
                U => "piston_top",
                D => "piston_bottom",
                _ => "piston_side",
            }
            .to_owned()
        });
    }
}
