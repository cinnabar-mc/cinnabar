//! Java Edition physics through the Bedrock-shaped simulator.
//!
//! On a Java session the simulator runs `java-sim` behind `PlayerState`, reading blocks by
//! the Bedrock state the core translated them to. These tests replay the recorded 1.7.10
//! client traces through that path, and check that translating a block and reading it back
//! keeps everything the physics depends on.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::Value;
use sim::{
    Aabb, CollisionQuery, CollisionWorld, MovementInput, PlayerState, Simulator, Vec3,
    WorldQueryError,
};

/// The core's table from 1.7.10 id and metadata to Bedrock state and hash.
const BLOCKS: &str = include_str!("../../../../core/javaedition/legacy/blocks.tsv");
const AIR_HASH: u32 = 3_690_217_760;

/// The network hash the core sends for each 1.7.10 id and metadata. Metadata the table does
/// not list takes the block's lowest listed metadata, as the core's lookup does.
fn translated_hashes() -> HashMap<(u16, u8), u32> {
    let mut listed: HashMap<(u16, u8), u32> = HashMap::new();
    let mut lowest: HashMap<u16, u32> = HashMap::new();
    for line in BLOCKS
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let fields: Vec<&str> = line.split('\t').collect();
        let (id, meta, hash): (u16, u8, u32) = (
            fields[0].parse().unwrap(),
            fields[1].parse().unwrap(),
            fields[3].parse().unwrap(),
        );
        listed.insert((id, meta), hash);
        lowest.entry(id).or_insert(hash);
    }
    for (&id, &hash) in &lowest {
        for meta in 0..16 {
            listed.entry((id, meta)).or_insert(hash);
        }
    }
    listed
}

/// A world of translated blocks: every column is loaded and unset positions are air.
struct TranslatedWorld {
    blocks: HashMap<[i32; 3], u32>,
}

impl CollisionWorld for TranslatedWorld {
    fn collision_boxes(&self, _query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(Vec::new()))
    }

    fn primary_block(&self, block: [i32; 3]) -> Result<Option<u32>, WorldQueryError> {
        Ok(Some(self.blocks.get(&block).copied().unwrap_or(AIR_HASH)))
    }
}

/// A `java-sim` world of one block at the origin.
struct Lone(java_sim::Block);

impl java_sim::World for Lone {
    fn block(&self, x: i32, y: i32, z: i32) -> java_sim::Block {
        if (x, y, z) == (0, 64, 0) {
            self.0
        } else {
            java_sim::Block::AIR
        }
    }

    fn chunk_loaded(&self, _chunk_x: i32, _chunk_z: i32) -> bool {
        true
    }
}

fn lone_boxes(block: java_sim::Block) -> Vec<[u64; 6]> {
    let mut boxes = Vec::new();
    let everywhere = java_sim::Aabb::new(-1e6, -1e6, -1e6, 1e6, 1e6, 1e6);
    java_sim::block::collision_boxes(&Lone(block), 0, 64, 0, &everywhere, &mut boxes);
    boxes
        .iter()
        .map(|b| [b.min_x, b.min_y, b.min_z, b.max_x, b.max_y, b.max_z].map(f64::to_bits))
        .collect()
}

/// The client only sees the Bedrock state. The block it recovers from that state must
/// collide and behave like the block the server sent, for every id and metadata.
#[test]
fn translated_blocks_keep_their_java_physics() {
    let hashes = translated_hashes();
    let mut lost = Vec::new();
    for (&(id, meta), &hash) in &hashes {
        let recovered = assets::facts::java_legacy::java_legacy_block(hash)
            .unwrap_or_else(|| panic!("{id}:{meta} translates to a state with no block behind it"));
        let (sent, read) = (
            java_sim::Block::new(id, meta),
            java_sim::Block::new(recovered.id, recovered.metadata),
        );
        let same_facts = java_sim::block::facts(sent.id) == java_sim::block::facts(read.id);
        let ladder_like =
            |id: u16| id == java_sim::block::id::LADDER || id == java_sim::block::id::VINE;
        let liquid_level = |block: java_sim::Block| {
            java_sim::block::facts(block.id)
                .liquid
                .then_some(block.meta)
        };
        if !same_facts
            || lone_boxes(sent) != lone_boxes(read)
            || ladder_like(sent.id) != ladder_like(read.id)
            || liquid_level(sent) != liquid_level(read)
            || (sent.id == java_sim::block::id::WEB) != (read.id == java_sim::block::id::WEB)
            || (sent.id == java_sim::block::id::SOUL_SAND)
                != (read.id == java_sim::block::id::SOUL_SAND)
        {
            lost.push((id, meta));
        }
    }
    lost.sort();
    let mut expected: Vec<(u16, u8)> = LOSSY
        .iter()
        .flat_map(|(id, metadata)| metadata.iter().map(|meta| (*id, *meta)))
        .collect();
    expected.sort();
    assert_eq!(
        lost, expected,
        "the blocks whose physics translation loses changed"
    );
}

/// Metadata whose physics the translation does not keep. Extended piston bases share their
/// retracted state and are recognised by the head in front of them instead, which the trace
/// replay covers. The rest is metadata the game never places: the core maps it to the
/// block's lowest listed metadata.
const LOSSY: &[(u16, &[u8])] = &[
    (29, &[8, 9, 10, 11, 12, 13]),
    (33, &[8, 9, 10, 11, 12, 13]),
    (34, &[6, 7, 14, 15]),
    (53, STAIRS),
    (65, &[0, 1, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]),
    (67, STAIRS),
    (78, &[9, 10, 11, 12, 13, 14, 15]),
    (92, &[7, 8, 9, 10, 11, 12, 13, 14, 15]),
    (107, &[8, 10]),
    (108, STAIRS),
    (109, STAIRS),
    (114, STAIRS),
    (120, &[12, 13, 14, 15]),
    (126, &[14, 15]),
    (127, &[12, 13, 14, 15]),
    (128, STAIRS),
    (134, STAIRS),
    (135, STAIRS),
    (136, STAIRS),
    (145, &[13, 15]),
    (156, STAIRS),
    (163, STAIRS),
    (164, STAIRS),
];

/// Stair metadata with the unused fourth bit set, beyond metadata 8, which equals 0.
const STAIRS: &[u8] = &[9, 10, 11, 12, 13, 14, 15];

fn double(value: &Value) -> f64 {
    f64::from_bits(value.as_i64().expect("f64 bits") as u64)
}

fn float(value: &Value) -> f32 {
    f32::from_bits(value.as_i64().expect("f32 bits") as u32)
}

fn flag(value: &Value) -> bool {
    value
        .as_bool()
        .unwrap_or_else(|| value.as_i64().expect("flag") != 0)
}

fn read_trace(path: &Path) -> String {
    let mut text = String::new();
    flate2::read::GzDecoder::new(std::fs::read(path).expect("trace").as_slice())
        .read_to_string(&mut text)
        .expect("gzip trace");
    text
}

/// Replays one trace through the simulator; `Ok(None)` for traces that use server events
/// the simulator's inputs do not carry (abilities, attribute snapshots, hunger).
fn replay(path: &Path, hashes: &HashMap<(u16, u8), u32>) -> Result<Option<usize>, String> {
    let text = read_trace(path);
    if [
        "\"abilities\":{\"allow_flying\":true",
        "\"attributes\"",
        "\"food\":",
    ]
    .iter()
    .any(|event| text.lines().skip(1).any(|line| line.contains(event)))
    {
        return Ok(None);
    }
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let header: Value = serde_json::from_str(lines.next().expect("header")).expect("header");
    if flag(&header["abilities"]["allow_flying"]) {
        return Ok(None);
    }
    let mut world = TranslatedWorld {
        blocks: HashMap::new(),
    };
    for block in header["blocks"].as_array().expect("blocks") {
        let number = |index: usize| block[index].as_i64().expect("block field");
        let hash = hashes[&(number(3) as u16, number(4) as u8)];
        world
            .blocks
            .insert([number(0) as i32, number(1) as i32, number(2) as i32], hash);
    }
    let start = &header["start"];
    let mut state = PlayerState::new(Vec3::new(
        double(&start["x"]),
        double(&start["y"]),
        double(&start["z"]),
    ));
    let simulator = Simulator::default();
    let mut jump_boost = None;
    let mut blindness = false;
    let mut ticks = 0;
    for line in lines {
        let tick: Value = serde_json::from_str(line).expect("tick");
        for event in tick["ev"].as_array().into_iter().flatten() {
            if let Some(velocity) = event.get("velocity") {
                state.velocity = Vec3::new(
                    double(&velocity[0]),
                    double(&velocity[1]),
                    double(&velocity[2]),
                );
            }
            if let Some(teleport) = event.get("teleport") {
                let feet = double(&teleport["y"]) - f64::from(java_sim::player::EYE_OFFSET);
                state.position = Vec3::new(double(&teleport["x"]), feet, double(&teleport["z"]));
            }
            if let Some(effect) = event.get("effect") {
                let amplifier = effect["amp"].as_i64().expect("amplifier") as i32;
                match effect["id"].as_i64() {
                    Some(8) => {
                        jump_boost =
                            Some(jump_boost.map_or(amplifier, |held: i32| held.max(amplifier)))
                    }
                    Some(15) => blindness = true,
                    _ => {}
                }
            }
            match event.get("remove_effect").and_then(Value::as_i64) {
                Some(8) => jump_boost = None,
                Some(15) => blindness = false,
                _ => {}
            }
        }
        let keys = &tick["in"];
        let axis = |positive: &str, negative: &str| {
            f64::from(i8::from(flag(&keys[positive])) - i8::from(flag(&keys[negative])))
        };
        let mut input = MovementInput {
            strafe: axis("l", "r"),
            forward: axis("f", "b"),
            yaw_degrees: f64::from(float(&keys["yaw"])),
            pitch_degrees: f64::from(float(&keys["pitch"])),
            jumping: flag(&keys["j"]),
            sneaking: flag(&keys["s"]),
            // The client's own rules decide sprinting on a Java session; the replay supplies
            // the state the recorded client held through the tick.
            sprinting: flag(&tick["out"]["sprint"]),
            java_physics: true,
            ..MovementInput::default()
        };
        input.using_consumable = flag(&keys["use"]);
        input.effects.jump_boost = jump_boost;
        input.effects.blindness = blindness;
        let result = simulator
            .tick(&mut state, input, &world)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        let out = &tick["out"];
        let expected = [
            double(&out["x"]),
            double(&out["miny"]),
            double(&out["z"]),
            double(&out["mx"]),
            double(&out["my"]),
            double(&out["mz"]),
        ];
        let actual = [
            result.position.x,
            result.position.y,
            result.position.z,
            result.velocity.x,
            result.velocity.y,
            result.velocity.z,
        ];
        if expected.map(f64::to_bits) != actual.map(f64::to_bits)
            || result.on_ground != flag(&out["g"])
        {
            return Err(format!(
                "{} tick {}: expected {expected:?} ground {}, simulated {actual:?} ground {}",
                path.display(),
                tick["t"],
                flag(&out["g"]),
                result.on_ground
            ));
        }
        ticks += 1;
    }
    Ok(Some(ticks))
}

/// Whether the read of a block fails, given how many reads came before it.
type Fails = fn([i32; 3], usize) -> bool;

/// A world whose block reads fail when `fails` says so.
struct FailingWorld {
    fails: Fails,
    reads: std::cell::Cell<usize>,
}

impl CollisionWorld for FailingWorld {
    fn collision_boxes(&self, _query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(Vec::new()))
    }

    fn primary_block(&self, block: [i32; 3]) -> Result<Option<u32>, WorldQueryError> {
        let read = self.reads.get();
        self.reads.set(read + 1);
        if (self.fails)(block, read) {
            Err(WorldQueryError::CoordinateOutOfRange)
        } else {
            Ok(Some(AIR_HASH))
        }
    }
}

/// A tick over a world it cannot read fails and leaves the state as it was, however many
/// reads failed and whether they were block reads or the loaded-column probe.
#[test]
fn a_java_tick_over_an_unreadable_world_fails_without_changing_the_state() {
    let input = MovementInput {
        java_physics: true,
        ..MovementInput::default()
    };
    let simulator = Simulator::default();
    let mut settled = PlayerState::new(Vec3::new(8.5, 64.0, 8.5));
    let readable = FailingWorld {
        fails: |_, _| false,
        reads: std::cell::Cell::new(0),
    };
    simulator
        .tick(&mut settled, input, &readable)
        .expect("a readable world ticks");
    let cases: [(&str, Fails); 4] = [
        ("one failed read", |_, read| read == 0),
        ("two failed reads", |_, read| read < 2),
        ("every read fails", |_, _| true),
        // The player stands at Y 64, so only the loaded-column probe reads Y 0.
        ("the column probe fails", |block, _| block[1] == 0),
    ];
    for (name, fails) in cases {
        let world = FailingWorld {
            fails,
            reads: std::cell::Cell::new(0),
        };
        let mut state = settled.clone();
        let result = simulator.tick(&mut state, input, &world);
        assert!(result.is_err(), "{name}: the tick succeeded");
        assert!(world.reads.get() > 0, "{name}: the world was never read");
        assert_eq!(state, settled, "{name}: the failed tick changed the state");
    }
}

/// The recorded client's positions, motion and ground flag come out of the simulator bit for
/// bit when the world is the core's translation of the recorded one.
#[test]
fn recorded_java_traces_replay_through_the_simulator() {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../java-sim/fixtures/traces");
    let mut traces: Vec<PathBuf> = std::fs::read_dir(&directory)
        .unwrap_or_else(|error| panic!("missing fixture: {}: {error}", directory.display()))
        .filter_map(|entry| Some(entry.ok()?.path()))
        .filter(|path| path.extension().is_some_and(|extension| extension == "gz"))
        .collect();
    traces.sort();
    let hashes = translated_hashes();
    let (mut replayed, mut failures) = (0, Vec::new());
    for trace in &traces {
        match replay(trace, &hashes) {
            Ok(Some(ticks)) => {
                assert!(ticks > 0);
                replayed += 1;
            }
            Ok(None) => {}
            Err(failure) => failures.push(failure),
        }
    }
    assert!(
        failures.is_empty(),
        "{} traces diverged:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert!(
        replayed >= 10,
        "only {replayed} of {} traces were replayed",
        traces.len()
    );
}
