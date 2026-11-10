//! Old guest record shapes and host-only physical gates; current imports drop these fields.

mod controls;

use super::{State, cinnabar::extension::gameplay};
use anyhow::Result;
use wasmtime::{
    Engine, StoreContextMut,
    component::{
        Component, ComponentType, Lift, Linker, Lower,
        types::{ComponentItem, Type},
    },
};

/// Host frame observation; the physical attack hold gates requests and never reaches guests.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub session: u64,
    pub dimension: i32,
    pub eye: gameplay::Vector3,
    pub yaw: f32,
    pub pitch: f32,
    pub attack_held: bool,
    pub frame_seconds: f32,
}

/// Host walking observation retaining physical gates for old guests and jump admission.
#[derive(Clone, Copy, Debug, PartialEq, ComponentType, Lower, Lift)]
#[component(record)]
pub struct MovementSnapshot {
    pub session: u64,
    pub dimension: i32,
    pub tick: u64,
    pub velocity: gameplay::Vector3,
    #[component(name = "on-ground")]
    pub on_ground: bool,
    #[component(name = "jump-held")]
    pub jump_held: bool,
    #[component(name = "sprint-held")]
    pub sprint_held: bool,
    #[component(name = "sneak-held")]
    pub sneak_held: bool,
    pub eligible: bool,
    #[component(name = "knockback-sequence")]
    pub knockback_sequence: u64,
}

impl From<Snapshot> for gameplay::Snapshot {
    fn from(value: Snapshot) -> Self {
        Self {
            session: value.session,
            dimension: value.dimension,
            eye: value.eye,
            yaw: value.yaw,
            pitch: value.pitch,
            frame_seconds: value.frame_seconds,
        }
    }
}
impl From<MovementSnapshot> for gameplay::MovementSnapshot {
    fn from(value: MovementSnapshot) -> Self {
        Self {
            session: value.session,
            dimension: value.dimension,
            tick: value.tick,
            velocity: value.velocity,
            on_ground: value.on_ground,
            eligible: value.eligible,
            knockback_sequence: value.knockback_sequence,
        }
    }
}

/// Detects the old result record; typed wrappers still reject every other signature mismatch.
fn has_field(item: Option<ComponentItem>, name: &str) -> bool {
    let Some(ComponentItem::ComponentFunc(func)) = item else {
        return false;
    };
    let Some(Type::Result(result)) = func.results().next() else {
        return false;
    };
    let Some(Type::Option(option)) = result.ok() else {
        return false;
    };
    let Type::Record(record) = option.ty() else {
        return false;
    };
    record.fields().any(|field| field.name == name)
}

/// Replaces the gameplay imports only for a built component that requested the old movement
/// record; every other import keeps its current type.
pub(super) fn link(
    engine: &Engine,
    component: &Component,
    linker: &mut Linker<State>,
) -> Result<()> {
    controls::link(engine, component, linker)?;
    for (name, item) in component.component_type().imports(engine) {
        if !name.starts_with("cinnabar:extension/gameplay@") {
            continue;
        }
        let ComponentItem::ComponentInstance(instance) = item else {
            continue;
        };
        if !has_field(instance.get_export(engine, "read-movement"), "jump-held") {
            continue;
        }
        linker.allow_shadowing(true);
        let mut imports = linker.instance(name)?;
        macro_rules! forward {
            ($wire:literal, $method:ident $(, $arg:ident : $ty:ty)*) => {
                imports.func_wrap($wire, |mut store: StoreContextMut<'_, State>, ($($arg,)*): ($($ty,)*)| {
                    Ok((gameplay::Host::$method(store.data_mut(), $($arg,)*)?,))
                })?;
            };
        }
        forward!("read-movement-input", read_movement_input);
        forward!("set-movement-input", set_movement_input, input: Option<gameplay::MovementInput>);
        forward!("cancel-jump", cancel_jump);
        forward!("pulse-jump", pulse_jump);
        forward!("set-movement-intents", set_movement_intents, intents: Option<gameplay::MovementIntents>);
        forward!("set-control-modes", set_control_modes, modes: Option<gameplay::ControlModes>);
        forward!("rotate", rotate, yaw: f32, pitch: f32);
        forward!("set-attack-reach", set_attack_reach, reach: Option<f32>);
        forward!("pulse-attack", pulse_attack);
        forward!("set-block-mining", set_block_mining, held: bool);
        forward!("set-packet-delay", set_packet_delay, delay: u32);
        forward!("read-relayed-position", read_relayed_position);
        forward!("read-camera", read_camera);
        forward!("set-camera-rig", set_camera_rig, rig: Option<gameplay::CameraRig>);
        forward!("request-command", request_command, command: String);
        forward!("read-frame", read_frame);
        imports.func_wrap(
            "read-movement",
            |mut store: StoreContextMut<'_, State>, (): ()| {
                Ok((super::gameplay::read_movement(store.data_mut())?,))
            },
        )?;
        linker.allow_shadowing(false);
    }
    Ok(())
}
