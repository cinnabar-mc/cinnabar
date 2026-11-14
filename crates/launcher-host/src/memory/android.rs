//! Android's `ActivityManager.MemoryInfo`, read through the application context.

use std::sync::atomic::{AtomicBool, Ordering};

use jni::{JNIEnv, objects::JObject};

use super::snapshot::PlatformMemory;

/// Local references one read creates: the service name, the manager and the info object.
const LOCAL_REFERENCES: i32 = 8;

/// What the activity manager reports now; `None`, logged once, when the call fails.
pub(super) fn memory_info() -> Option<PlatformMemory> {
    static WARNED: AtomicBool = AtomicBool::new(false);
    match read() {
        Ok(memory) => Some(memory),
        Err(error) => {
            if !WARNED.swap(true, Ordering::Relaxed) {
                tracing::warn!("Android memory information is unavailable: {error}");
            }
            None
        }
    }
}

/// Attaches the calling thread for the call and clears any Java exception it raised.
fn read() -> jni::errors::Result<PlatformMemory> {
    let context = ndk_context::android_context();
    // The Android runtime owns the VM and the application context for the process lifetime.
    let vm = unsafe { jni::JavaVM::from_raw(context.vm().cast())? };
    let mut env = vm.attach_current_thread()?;
    // android-activity publishes the Application here, not the hosting activity.
    let application = unsafe { JObject::from_raw(context.context().cast()) };
    let result = env.with_local_frame(LOCAL_REFERENCES, |env| query(env, &application));
    if result.is_err() && env.exception_check()? {
        env.exception_clear()?;
    }
    result
}

/// `((ActivityManager) context.getSystemService("activity")).getMemoryInfo(info)`.
fn query(env: &mut JNIEnv, context: &JObject) -> jni::errors::Result<PlatformMemory> {
    let service = env.new_string("activity")?;
    let manager = env
        .call_method(
            context,
            "getSystemService",
            "(Ljava/lang/String;)Ljava/lang/Object;",
            &[(&service).into()],
        )?
        .l()?;
    let info = env.new_object("android/app/ActivityManager$MemoryInfo", "()V", &[])?;
    env.call_method(
        &manager,
        "getMemoryInfo",
        "(Landroid/app/ActivityManager$MemoryInfo;)V",
        &[(&info).into()],
    )?;
    Ok(PlatformMemory {
        available: env.get_field(&info, "availMem", "J")?.j()?.max(0) as u64,
        threshold: env.get_field(&info, "threshold", "J")?.j()?.max(0) as u64,
        low: env.get_field(&info, "lowMemory", "Z")?.z()?,
    })
}
