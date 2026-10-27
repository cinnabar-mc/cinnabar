//! The material showcase's client part: it restyles the Experience's own entities and blocks
//! with programmable world materials and follows the server's commands on
//! `materialfx.control`. Every server-driven change (a shockwave, a hit, the radar, the layout)
//! redeclares a material with the same shader and new parameters, which the host applies
//! without recompiling.

mod effects;

use experience_sdk::Value;
use experience_sdk::client::ClientPart;
use experience_sdk::client::world_material as wm;

use effects::{EFFECTS, MOBS, State, THERMAL};

/// The declarations of `experience.toml`, which `build.rs` writes.
mod experience {
    experience_sdk::include_declarations!();
}

/// The control channel's operations, numbered as `experience.toml` lists them; `build.rs`
/// writes them.
mod operations {
    include!(concat!(env!("OUT_DIR"), "/operations.rs"));
}

// The control channel's choice admits exactly the operations the manifest numbers.
const _: () = assert!(matches!(
    experience::channels::CONTROL.fields.first(),
    Some(experience_sdk::Field::Choice { variants }) if *variants == operations::COUNT
));

std::thread_local! {
    /// The showcase's state, which the parameters are computed from.
    static STATE: std::cell::RefCell<State> = std::cell::RefCell::new(State::default());
}

/// Declares material `id` with the current parameters.
fn declare(id: u32) -> Result<(), String> {
    let spec = STATE.with_borrow(|state| effects::spec(id, state));
    wm::set_material(id, &spec)
}

/// Declares every material not yet declared, as far as this callback's compile budget goes,
/// and binds each declared one to its target; mobs bind to thermal vision while it is on.
fn bind_all() {
    for id in 0..EFFECTS.len() as u32 {
        let declared = STATE.with_borrow(|state| state.declared & (1 << id) != 0);
        if !declared {
            if declare(id).is_err() {
                continue;
            }
            STATE.with_borrow_mut(|state| state.declared |= 1 << id);
        }
    }
    let (declared, thermal) = STATE.with_borrow(|state| (state.declared, state.thermal));
    for (id, effect) in EFFECTS.iter().enumerate() {
        let id = id as u32;
        let Some(target) = (effect.target)() else {
            continue;
        };
        let material = if thermal && MOBS.contains(&id) && declared & (1 << THERMAL) != 0 {
            THERMAL
        } else {
            id
        };
        if declared & (1 << material) != 0 {
            let _ = wm::bind(id, &target, material);
        }
    }
}

/// Unbinds every target, so each draws with its Bedrock material again.
fn unbind_all() {
    for id in 0..EFFECTS.len() as u32 {
        let _ = wm::unbind(id);
    }
}

/// Redeclares the materials `ids` with their new parameters, those already declared.
fn refresh(ids: &[u32]) {
    let declared = STATE.with_borrow(|state| state.declared);
    for &id in ids {
        if declared & (1 << id) != 0 {
            let _ = declare(id);
        }
    }
}

struct MaterialFx;

impl ClientPart for MaterialFx {
    fn init() {
        bind_all();
    }
}

impl experience_sdk::client::__private::Dispatch for MaterialFx {
    /// Applies one showcase control record from the server half.
    fn dispatch(channel: String, record: Vec<Value>) -> Result<(), String> {
        if channel != experience::channels::CONTROL.id {
            return Ok(());
        }
        let Some(Value::Choice(op)) = record.first() else {
            return Ok(());
        };
        let argument = |index: usize| match record.get(index) {
            Some(Value::Integer(value)) => *value,
            _ => 0,
        };
        let args = [argument(1), argument(2), argument(3), argument(4)];
        match *op {
            operations::BIND => bind_all(),
            operations::UNBIND => unbind_all(),
            operations::THERMAL => {
                STATE.with_borrow_mut(|state| state.thermal = args[0] == 1);
                bind_all();
            }
            op => {
                let changed = STATE.with_borrow_mut(|state| state.apply(op, args));
                refresh(changed);
            }
        }
        Ok(())
    }
}

experience_sdk::export_client_part!(MaterialFx);
