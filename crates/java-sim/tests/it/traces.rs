//! Replays movement traces recorded from the 1.7.10 client and requires bit-identical results.
//!
//! Each trace is a JSONL file: a header with the world and start state, then one line per tick
//! with the input, the events applied before it, and the client's resulting state and packets.
//! Floating-point values are stored as their raw bit patterns.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use java_sim::player::{Modifier, Operation};
use java_sim::{Abilities, Block, Input, Packet, Player, Status, World};
use serde_json::Value;

/// A sparse block world; every column is loaded and unset positions are air.
#[derive(Default)]
struct TraceWorld {
    blocks: HashMap<(i32, i32, i32), Block>,
}

impl World for TraceWorld {
    fn block(&self, x: i32, y: i32, z: i32) -> Block {
        self.blocks.get(&(x, y, z)).copied().unwrap_or(Block::AIR)
    }

    fn chunk_loaded(&self, _chunk_x: i32, _chunk_z: i32) -> bool {
        true
    }
}

/// Reads a JSON integer holding an `f64` bit pattern.
fn double(value: &Value) -> f64 {
    f64::from_bits(value.as_i64().expect("f64 bits") as u64)
}

/// Reads a JSON integer holding an `f32` bit pattern.
fn float(value: &Value) -> f32 {
    f32::from_bits(value.as_i64().expect("f32 bits") as u32)
}

fn flag(value: &Value) -> bool {
    match value {
        Value::Bool(flag) => *flag,
        other => other.as_i64().expect("flag") != 0,
    }
}

fn abilities(value: &Value) -> Abilities {
    Abilities {
        invulnerable: flag(&value["invulnerable"]),
        flying: flag(&value["flying"]),
        allow_flying: flag(&value["allow_flying"]),
        creative: flag(&value["creative"]),
        fly_speed: float(&value["fly_speed"]),
        walk_speed: float(&value["walk_speed"]),
    }
}

/// Converts a recorded packet to the simulation's packet, or `None` for packets the movement
/// simulation does not produce.
fn recorded_packet(value: &Value) -> Option<Packet> {
    let on_ground = || flag(&value["g"]);
    Some(match value["k"].as_str()? {
        "action" => Packet::Action(value["a"].as_i64()? as i32),
        "ground" => Packet::Ground {
            on_ground: on_ground(),
        },
        "pos" => Packet::Position {
            x: double(&value["x"]),
            y: double(&value["y"]),
            stance: double(&value["stance"]),
            z: double(&value["z"]),
            on_ground: on_ground(),
        },
        "look" => Packet::Look {
            yaw: float(&value["yaw"]),
            pitch: float(&value["pitch"]),
            on_ground: on_ground(),
        },
        "poslook" => Packet::PositionLook {
            x: double(&value["x"]),
            y: double(&value["y"]),
            stance: double(&value["stance"]),
            z: double(&value["z"]),
            yaw: float(&value["yaw"]),
            pitch: float(&value["pitch"]),
            on_ground: on_ground(),
        },
        "abilities" => {
            let flags = value["flags"].as_i64()?;
            Packet::Abilities {
                invulnerable: flags & 1 != 0,
                flying: flags & 2 != 0,
                allow_flying: flags & 4 != 0,
                creative: flags & 8 != 0,
                fly_speed: float(&value["fly"]),
                walk_speed: float(&value["walk"]),
            }
        }
        _ => return None,
    })
}

/// A packet as its kind, flags and floating-point bit patterns, so `-0.0` differs from `0.0`.
fn packet_bits(packet: &Packet) -> (u8, Vec<u64>) {
    let single = |value: f32| u64::from(value.to_bits());
    match *packet {
        Packet::Action(action) => (0, vec![action as u64]),
        Packet::Ground { on_ground } => (1, vec![u64::from(on_ground)]),
        Packet::Position {
            x,
            y,
            stance,
            z,
            on_ground,
        } => (
            2,
            vec![
                x.to_bits(),
                y.to_bits(),
                stance.to_bits(),
                z.to_bits(),
                u64::from(on_ground),
            ],
        ),
        Packet::Look {
            yaw,
            pitch,
            on_ground,
        } => (3, vec![single(yaw), single(pitch), u64::from(on_ground)]),
        Packet::PositionLook {
            x,
            y,
            stance,
            z,
            yaw,
            pitch,
            on_ground,
        } => (
            4,
            vec![
                x.to_bits(),
                y.to_bits(),
                stance.to_bits(),
                z.to_bits(),
                single(yaw),
                single(pitch),
                u64::from(on_ground),
            ],
        ),
        Packet::Abilities {
            invulnerable,
            flying,
            allow_flying,
            creative,
            fly_speed,
            walk_speed,
        } => (
            5,
            vec![
                u64::from(invulnerable),
                u64::from(flying),
                u64::from(allow_flying),
                u64::from(creative),
                single(fly_speed),
                single(walk_speed),
            ],
        ),
    }
}

#[test]
fn packet_comparison_tells_signed_zeros_apart() {
    let packet = |x: f64| Packet::Position {
        x,
        y: 0.0,
        stance: 0.0,
        z: 0.0,
        on_ground: true,
    };
    assert_eq!(
        packet(0.0),
        packet(-0.0),
        "float equality cannot see the sign"
    );
    assert_ne!(packet_bits(&packet(0.0)), packet_bits(&packet(-0.0)));
}

fn recorded_packets(value: &Value) -> Vec<Packet> {
    value
        .as_array()
        .map(|packets| packets.iter().filter_map(recorded_packet).collect())
        .unwrap_or_default()
}

/// The comparable state after a tick, as bit patterns so `-0.0` and `0.0` differ.
#[derive(Debug, PartialEq)]
struct Snapshot {
    position: [u64; 4],
    motion: [u64; 3],
    flags: [bool; 7],
    fall: u32,
    sink: u32,
}

fn snapshot(player: &Player) -> Snapshot {
    let [x, feet, z] = player.feet();
    Snapshot {
        position: [
            x.to_bits(),
            player.stance().to_bits(),
            feet.to_bits(),
            z.to_bits(),
        ],
        motion: player.motion().map(f64::to_bits),
        flags: [
            player.on_ground(),
            player.collided_horizontally(),
            player.collided_vertically(),
            player.sprinting(),
            player.sneaking(),
            player.in_water(),
            player.abilities().flying,
        ],
        fall: player.fall_distance().to_bits(),
        sink: player.sink().to_bits(),
    }
}

fn recorded_snapshot(out: &Value) -> Snapshot {
    let bits = |key: &str| out[key].as_i64().expect("f64 bits") as u64;
    Snapshot {
        position: [bits("x"), bits("y"), bits("miny"), bits("z")],
        motion: [bits("mx"), bits("my"), bits("mz")],
        flags: [
            flag(&out["g"]),
            flag(&out["ch"]),
            flag(&out["cv"]),
            flag(&out["sprint"]),
            flag(&out["sneak"]),
            flag(&out["water"]),
            flag(&out["flying"]),
        ],
        fall: out["fall"].as_i64().expect("f32 bits") as u32,
        sink: out["ysize"].as_i64().expect("f32 bits") as u32,
    }
}

/// Renders a snapshot with decimal values for a failure message.
fn describe(snapshot: &Snapshot) -> String {
    format!(
        "pos {:?} motion {:?} flags(g,ch,cv,sprint,sneak,water,fly) {:?} fall {} sink {}",
        snapshot.position.map(f64::from_bits),
        snapshot.motion.map(f64::from_bits),
        snapshot.flags,
        f32::from_bits(snapshot.fall),
        f32::from_bits(snapshot.sink),
    )
}

/// Applies the events recorded before a tick; returns the packets the client sent in reply.
fn apply_events(
    player: &mut Player,
    status: &mut Status,
    events: &Value,
    expected: &mut Vec<Packet>,
) -> Vec<Packet> {
    let mut sent = Vec::new();
    for event in events.as_array().into_iter().flatten() {
        if let Some(velocity) = event.get("velocity") {
            player.set_velocity(
                double(&velocity[0]),
                double(&velocity[1]),
                double(&velocity[2]),
            );
        }
        if let Some(teleport) = event.get("teleport") {
            sent.push(player.teleport(
                double(&teleport["x"]),
                double(&teleport["y"]),
                double(&teleport["z"]),
                float(&teleport["yaw"]),
                float(&teleport["pitch"]),
                teleport.get("g").is_some_and(flag),
            ));
            expected.extend(recorded_packets(&event["pk"]));
        }
        if let Some(effect) = event.get("effect") {
            let id = effect["id"].as_i64().expect("effect id") as u8;
            status
                .effects
                .apply(id, effect["amp"].as_i64().expect("amplifier") as i8);
        }
        if let Some(removed) = event.get("remove_effect") {
            status
                .effects
                .remove(removed.as_i64().expect("effect id") as u8);
        }
        if let Some(value) = event.get("abilities") {
            player.set_abilities(abilities(value));
        }
        if let Some(food) = event.get("food") {
            status.food_level = food.as_i64().expect("food") as i32;
        }
        if let Some(attributes) = event.get("attributes") {
            let modifiers: Vec<Modifier> = attributes["mods"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|modifier| {
                    let high = modifier["uuid"][0].as_i64()? as u64;
                    let low = modifier["uuid"][1].as_i64()? as u64;
                    Some(Modifier {
                        id: (u128::from(high) << 64) | u128::from(low),
                        amount: double(&modifier["amount"]),
                        operation: Operation::from_wire(modifier["op"].as_i64()? as u8)?,
                    })
                })
                .collect();
            player
                .movement_speed_mut()
                .apply_snapshot(double(&attributes["base"]), &modifiers);
        }
    }
    sent
}

/// Replays one trace; returns a description of the first divergence, if any.
fn replay(path: &Path) -> Result<usize, String> {
    let text =
        crate::fixtures::read(path).ok_or_else(|| format!("{}: unreadable", path.display()))?;
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let header: Value = serde_json::from_str(lines.next().ok_or("empty trace")?)
        .map_err(|error| error.to_string())?;
    let mut world = TraceWorld::default();
    for block in header["blocks"].as_array().into_iter().flatten() {
        let number = |index: usize| block[index].as_i64().expect("block field");
        world.blocks.insert(
            (number(0) as i32, number(1) as i32, number(2) as i32),
            Block::new(number(3) as u16, number(4) as u8),
        );
    }
    let start = &header["start"];
    let mut player = Player::new(
        double(&start["x"]),
        double(&start["posy"]),
        double(&start["z"]),
        float(&start["yaw"]),
        float(&start["pitch"]),
    );
    player.set_abilities(abilities(&header["abilities"]));
    let mut status = Status {
        food_level: header["food"].as_i64().unwrap_or(20) as i32,
        ..Status::default()
    };
    let mut ticks = 0;
    for line in lines {
        let tick: Value = serde_json::from_str(line).map_err(|error| error.to_string())?;
        let number = tick["t"].as_i64().unwrap_or(-1);
        let mut expected_packets = Vec::new();
        let mut packets =
            apply_events(&mut player, &mut status, &tick["ev"], &mut expected_packets);
        let keys = &tick["in"];
        let input = Input {
            forward: flag(&keys["f"]),
            back: flag(&keys["b"]),
            left: flag(&keys["l"]),
            right: flag(&keys["r"]),
            jump: flag(&keys["j"]),
            sneak: flag(&keys["s"]),
            sprint: flag(&keys["p"]),
            yaw: float(&keys["yaw"]),
            pitch: float(&keys["pitch"]),
        };
        status.using_item = flag(&keys["use"]);
        player.tick(&world, &input, &status, &mut packets);
        expected_packets.extend(recorded_packets(&tick["out"]["pk"]));
        let (actual, expected) = (snapshot(&player), recorded_snapshot(&tick["out"]));
        if actual != expected {
            return Err(format!(
                "{} tick {number}: state diverged\n  expected {}\n  actual   {}",
                path.display(),
                describe(&expected),
                describe(&actual)
            ));
        }
        if packets.len() != expected_packets.len()
            || packets
                .iter()
                .zip(&expected_packets)
                .any(|(sent, recorded)| packet_bits(sent) != packet_bits(recorded))
        {
            return Err(format!(
                "{} tick {number}: packets diverged\n  expected {expected_packets:?}\n  actual   {packets:?}",
                path.display()
            ));
        }
        ticks += 1;
    }
    Ok(ticks)
}

fn trace_files() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(crate::fixtures::directory().join("traces"))
        .into_iter()
        .flatten()
        .filter_map(|entry| Some(entry.ok()?.path()))
        .filter(|path| path.extension().is_some_and(|extension| extension == "gz"))
        .collect();
    files.sort();
    files
}

#[test]
fn recorded_client_traces_replay_bit_for_bit() {
    let files = trace_files();
    assert!(
        !files.is_empty(),
        "missing fixture: crates/java-sim/fixtures/traces/*.jsonl.gz"
    );
    let mut failures = Vec::new();
    for file in &files {
        match replay(file) {
            Ok(ticks) => assert!(ticks > 0, "{} holds no ticks", file.display()),
            Err(failure) => failures.push(failure),
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} traces diverged:\n{}",
        failures.len(),
        files.len(),
        failures.join("\n")
    );
}
