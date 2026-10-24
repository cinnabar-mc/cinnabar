//! The material showcase's client part: it restyles the Experience's own entities and blocks
//! with five programmable world materials and follows the server's commands on
//! `materialfx.control` to revoke and restore them, flash damage, tint a status and hold the
//! dissolve cut. Parameter changes redeclare a material with the same shader, which the host
//! applies without recompiling.

use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};

use experience_sdk::Value;
use experience_sdk::client::ClientPart;
use experience_sdk::client::world_material::{self as wm, Blend, Depth, MaterialSpec, Target};

/// The declarations of `experience.toml`, which `build.rs` writes.
mod experience {
    experience_sdk::include_declarations!();
}

/// One showcase material: its shader, the textures and initial parameters it declares, its
/// host blend class and the target it restyles.
struct Effect {
    source: &'static str,
    textures: &'static [&'static str],
    blend: Blend,
    target: fn() -> Target,
}

/// Material and binding ids, which index [`EFFECTS`].
const DISSOLVE: u32 = 0;
const HOLOGRAM: u32 = 1;
const ENERGY: u32 = 2;
const STATUS: u32 = 3;
const PORTAL: u32 = 4;

/// The showcase's materials, by id.
const EFFECTS: [Effect; 5] = [
    Effect {
        source: include_str!("../shaders/dissolve.wgsl"),
        textures: &["textures/noise.png"],
        blend: Blend::Cutout,
        target: || Target::Entity("materialfx:wraith".into()),
    },
    Effect {
        source: include_str!("../shaders/hologram.wgsl"),
        textures: &[],
        blend: Blend::Additive,
        target: || Target::Entity("materialfx:hologram".into()),
    },
    Effect {
        source: include_str!("../shaders/energy.wgsl"),
        textures: &[],
        blend: Blend::Opaque,
        target: || Target::Block("materialfx:energy".into()),
    },
    Effect {
        source: include_str!("../shaders/status.wgsl"),
        textures: &[],
        blend: Blend::Opaque,
        target: || Target::Entity("materialfx:sentinel".into()),
    },
    Effect {
        source: include_str!("../shaders/portal.wgsl"),
        textures: &["textures/stars.png"],
        blend: Blend::Opaque,
        target: || Target::Block("materialfx:portal".into()),
    },
];

/// Status colours by the server's status value: none, poison, burning, frozen.
const STATUS_COLOURS: [[f32; 3]; 4] = [
    [0.0, 0.0, 0.0],
    [0.35, 1.0, 0.2],
    [1.0, 0.45, 0.05],
    [0.45, 0.8, 1.0],
];

/// Materials declared so far, one bit per id; the host keeps them across callbacks.
static DECLARED: AtomicU32 = AtomicU32::new(0);
/// The current damage flash, in thousandths.
static FLASH: AtomicI32 = AtomicI32::new(0);
/// The current status value.
static STATUS_VALUE: AtomicI32 = AtomicI32::new(0);
/// The held dissolve cut in thousandths, or -1 to sweep.
static CUT: AtomicI32 = AtomicI32::new(-1);

/// Material `id`'s current parameters.
fn params(id: u32) -> Vec<f32> {
    match id {
        DISSOLVE => {
            let cut = CUT.load(Ordering::Relaxed);
            let cut = if cut < 0 { -1.0 } else { cut as f32 / 1000.0 };
            vec![0.08, 1.0, 0.42, 0.08, 6.0, cut]
        }
        HOLOGRAM => vec![0.25, 0.85, 1.0, 6.0],
        ENERGY => vec![1.0, 3.5],
        STATUS => {
            let status = STATUS_VALUE.load(Ordering::Relaxed).clamp(0, 3) as usize;
            let [r, g, b] = STATUS_COLOURS[status];
            let strength = if status == 0 { 0.0 } else { 1.0 };
            let flash = FLASH.load(Ordering::Relaxed) as f32 / 1000.0;
            vec![r, g, b, strength, flash]
        }
        PORTAL => vec![1.5, 0.45, 0.15, 0.85],
        _ => Vec::new(),
    }
}

/// Declares material `id` with its current parameters.
fn declare(id: u32) -> Result<(), String> {
    let effect = &EFFECTS[id as usize];
    wm::set_material(
        id,
        &MaterialSpec {
            source: effect.source.into(),
            textures: effect.textures.iter().map(|path| (*path).into()).collect(),
            params: params(id),
            depth: Depth::Test,
            blend: effect.blend,
        },
    )
}

/// Declares every material not yet declared, as far as this callback's compile budget goes,
/// and binds each declared one to its target.
fn bind_all() {
    for id in 0..EFFECTS.len() as u32 {
        let bit = 1 << id;
        if DECLARED.load(Ordering::Relaxed) & bit == 0 {
            if declare(id).is_err() {
                continue;
            }
            DECLARED.fetch_or(bit, Ordering::Relaxed);
        }
        let _ = wm::bind(id, &(EFFECTS[id as usize].target)(), id);
    }
}

/// Unbinds every target, so each draws with its Bedrock material again.
fn unbind_all() {
    for id in 0..EFFECTS.len() as u32 {
        let _ = wm::unbind(id);
    }
}

struct MaterialFx;

impl ClientPart for MaterialFx {
    fn init() {
        bind_all();
    }

    fn dispatch(channel: String, record: Vec<Value>) {
        if channel != experience::channels::CONTROL.id {
            return;
        }
        let (Some(Value::Choice(op)), Some(Value::Integer(value))) =
            (record.first(), record.get(1))
        else {
            return;
        };
        let value = (*value).clamp(-1, 1000) as i32;
        match op {
            0 => bind_all(),
            1 => unbind_all(),
            2 => {
                FLASH.store(value.max(0), Ordering::Relaxed);
                let _ = declare(STATUS);
            }
            3 => {
                STATUS_VALUE.store(value, Ordering::Relaxed);
                let _ = declare(STATUS);
            }
            _ => {
                CUT.store(value, Ordering::Relaxed);
                let _ = declare(DISSOLVE);
            }
        }
    }
}

experience_sdk::export_client_part!(MaterialFx);
