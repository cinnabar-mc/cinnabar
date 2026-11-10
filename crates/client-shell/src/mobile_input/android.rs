//! Android controllers through winit's input filter.

use std::{
    collections::HashMap,
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
};

use bevy::{ecs::system::NonSendMarker, prelude::*};
use jni::{JNIEnv, objects::JObject};
use winit::platform::android::activity::input::{Axis, InputEvent, KeyAction};

use super::{
    android_map::{
        ControllerState, JoystickSample, SOURCE_CLASS_JOYSTICK, SOURCE_GAMEPAD, SOURCE_JOYSTICK,
        controller_button,
    },
    gamepads::{self, NativeGamepadEvent, NativeGamepads, PendingNativeGamepadEvents},
};

/// Frames between controller hotplug scans.
const DEVICE_SCAN_FRAMES: u32 = 60;

/// Per-device controller state owned by the input filter on the activity's input thread.
static CONTROLLERS: Mutex<Option<HashMap<u64, ControllerState>>> = Mutex::new(None);
/// Set when the filter sees a device without state, so the next frame scans devices and
/// names it before its input is forwarded.
static NEW_DEVICE: AtomicBool = AtomicBool::new(true);

/// Installs the controller filter and forwards controller input.
pub(super) fn configure(app: &mut App) {
    if !winit::platform::android::set_input_event_filter(filter) {
        warn!("Another Android input filter is installed; controllers are unavailable");
    }
    gamepads::configure(app);
    app.add_systems(
        PreUpdate,
        scan_devices
            .before(gamepads::NativeGamepadInput)
            .before(bevy::input::InputSystems),
    );
}

/// Consumes controller key and joystick events; everything else stays with winit.
fn filter(event: &InputEvent<'_>) -> bool {
    match event {
        InputEvent::KeyEvent(key) => {
            let Some(button) = controller_button(key.key_code().into(), key.source().into()) else {
                return false;
            };
            let pressed = match key.action() {
                KeyAction::Down if key.repeat_count() == 0 => true,
                KeyAction::Up => false,
                _ => return true,
            };
            let id = device(key.device_id());
            with_controller(id, |state| gamepads::queue(state.key(id, button, pressed)));
            true
        }
        InputEvent::MotionEvent(motion) => {
            if u32::from(motion.source()) & SOURCE_CLASS_JOYSTICK == 0 {
                return false;
            }
            let pointer = motion.pointer_at_index(0);
            let sample = JoystickSample {
                x: pointer.axis_value(Axis::X),
                y: pointer.axis_value(Axis::Y),
                z: pointer.axis_value(Axis::Z),
                rz: pointer.axis_value(Axis::Rz),
                ltrigger: pointer.axis_value(Axis::Ltrigger),
                rtrigger: pointer.axis_value(Axis::Rtrigger),
                brake: pointer.axis_value(Axis::Brake),
                gas: pointer.axis_value(Axis::Gas),
                hat_x: pointer.axis_value(Axis::HatX),
                hat_y: pointer.axis_value(Axis::HatY),
            };
            let id = device(motion.device_id());
            with_controller(id, |state| gamepads::queue(state.joystick(id, sample)));
            true
        }
        _ => false,
    }
}

/// Android device ids are non-negative `int`s.
fn device(id: i32) -> u64 {
    u64::from(id.cast_unsigned())
}

fn with_controller(id: u64, apply: impl FnOnce(&mut ControllerState)) {
    let mut controllers = CONTROLLERS.lock().unwrap_or_else(PoisonError::into_inner);
    let state = controllers
        .get_or_insert_with(HashMap::new)
        .entry(id)
        .or_insert_with(|| {
            NEW_DEVICE.store(true, Ordering::Release);
            ControllerState::default()
        });
    apply(state);
}

/// A connected controller as `InputDevice` describes it.
struct Controller {
    id: u64,
    name: String,
    vendor_id: u16,
    product_id: u16,
}

/// Reports controller connections and removals, naming new devices before their input.
/// Only devices an earlier scan listed are disconnected by a later one, so a controller the
/// scan cannot classify keeps the connection its first input made.
fn scan_devices(
    gamepads: Res<NativeGamepads>,
    mut pending: ResMut<PendingNativeGamepadEvents>,
    mut frames: Local<u32>,
    mut listed: Local<Vec<u64>>,
    _main_thread: NonSendMarker,
) {
    *frames = frames.wrapping_add(1);
    if !NEW_DEVICE.swap(false, Ordering::AcqRel) && !frames.is_multiple_of(DEVICE_SCAN_FRAMES) {
        return;
    }
    let controllers = match with_activity(|env, _| connected_controllers(env)) {
        Ok(controllers) => controllers,
        Err(error) => {
            static WARNING: std::sync::Once = std::sync::Once::new();
            WARNING.call_once(|| warn!("Cannot list Android controllers: {error}"));
            return;
        }
    };
    let removed = listed
        .iter()
        .copied()
        .filter(|id| !controllers.iter().any(|controller| controller.id == *id))
        .collect::<Vec<_>>();
    if !removed.is_empty() {
        let mut states = CONTROLLERS.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(states) = states.as_mut() {
            for id in &removed {
                states.remove(id);
            }
        }
    }
    *listed = controllers.iter().map(|controller| controller.id).collect();
    let connected = gamepads.ids().collect::<Vec<_>>();
    let mut events = controllers
        .into_iter()
        .filter(|controller| !connected.contains(&controller.id))
        .map(|controller| NativeGamepadEvent::Connected {
            id: controller.id,
            name: controller.name,
            vendor_id: Some(controller.vendor_id),
            product_id: Some(controller.product_id),
        })
        .chain(
            removed
                .into_iter()
                .map(|id| NativeGamepadEvent::Disconnected { id }),
        )
        .collect::<Vec<_>>();
    // Connections precede input the filter queued for the same devices.
    events.append(&mut pending.0);
    pending.0 = events;
}

/// Physical input devices with gamepad or joystick sources.
fn connected_controllers(env: &mut JNIEnv) -> jni::errors::Result<Vec<Controller>> {
    const DEVICE: &str = "android/view/InputDevice";
    let ids = env
        .call_static_method(DEVICE, "getDeviceIds", "()[I", &[])?
        .l()?;
    let ids = jni::objects::JIntArray::from(ids);
    let mut buffer = vec![0; usize::try_from(env.get_array_length(&ids)?).unwrap_or(0)];
    env.get_int_array_region(&ids, 0, &mut buffer)?;
    let mut controllers = Vec::new();
    for id in buffer {
        let input_device = env
            .call_static_method(
                DEVICE,
                "getDevice",
                "(I)Landroid/view/InputDevice;",
                &[id.into()],
            )?
            .l()?;
        if input_device.is_null()
            || env
                .call_method(&input_device, "isVirtual", "()Z", &[])?
                .z()?
        {
            continue;
        }
        let sources = env
            .call_method(&input_device, "getSources", "()I", &[])?
            .i()?;
        let sources = sources.cast_unsigned();
        if sources & SOURCE_GAMEPAD != SOURCE_GAMEPAD
            && sources & SOURCE_JOYSTICK != SOURCE_JOYSTICK
        {
            continue;
        }
        let name = env
            .call_method(&input_device, "getName", "()Ljava/lang/String;", &[])?
            .l()?;
        let name = env.get_string(&jni::objects::JString::from(name))?.into();
        let vendor_id = env
            .call_method(&input_device, "getVendorId", "()I", &[])?
            .i()?;
        let product_id = env
            .call_method(&input_device, "getProductId", "()I", &[])?
            .i()?;
        controllers.push(Controller {
            id: device(id),
            name,
            vendor_id: u16::try_from(vendor_id).unwrap_or(0),
            product_id: u16::try_from(product_id).unwrap_or(0),
        });
    }
    Ok(controllers)
}

/// Runs a JNI call with the client activity, clearing any Java exception it raises.
fn with_activity<T>(
    call: impl FnOnce(&mut JNIEnv, &JObject) -> jni::errors::Result<T>,
) -> jni::errors::Result<T> {
    let app = bevy::android::ANDROID_APP
        .get()
        .ok_or(jni::errors::Error::NullPtr(
            "Android activity is unavailable",
        ))?;
    // Bevy keeps the VM and this activity's global reference alive for the native loop.
    let vm = unsafe { jni::JavaVM::from_raw(app.vm_as_ptr().cast())? };
    let mut env = vm.attach_current_thread()?;
    let activity = unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
    let result = env.with_local_frame(32, |env| call(env, &activity));
    if result.is_err() && env.exception_check()? {
        env.exception_clear()?;
    }
    result
}
