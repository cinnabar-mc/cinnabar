//! Browser and document-picker handoff through Android intents, since Android has no desktop
//! opener or picker tools.

use jni::{
    JNIEnv,
    objects::{JClass, JObject, JObjectArray, JString},
};
use std::path::PathBuf;

/// `Intent.FLAG_ACTIVITY_NEW_TASK`, required to start an activity from the application context.
const FLAG_ACTIVITY_NEW_TASK: i32 = 0x1000_0000;

/// Starts a browsable `ACTION_VIEW` intent; `Ok(false)` means no app accepted the link.
pub(super) fn open_url(url: &str) -> jni::errors::Result<bool> {
    let context = ndk_context::android_context();
    // The Android runtime owns the VM and the application context for the process lifetime.
    let vm = unsafe { jni::JavaVM::from_raw(context.vm().cast())? };
    let mut env = vm.attach_current_thread()?;
    // android-activity publishes the Application here, not the hosting activity.
    let application = unsafe { JObject::from_raw(context.context().cast()) };
    let result = start_view_intent(&mut env, &application, url);
    if result.is_err() && env.exception_check()? {
        env.exception_clear()?;
        return Ok(false);
    }
    result.map(|()| true)
}

/// Builds `new Intent(ACTION_VIEW, Uri.parse(url))` and starts it in a new task.
fn start_view_intent(env: &mut JNIEnv, context: &JObject, url: &str) -> jni::errors::Result<()> {
    let url = env.new_string(url)?;
    let uri = env
        .call_static_method(
            "android/net/Uri",
            "parse",
            "(Ljava/lang/String;)Landroid/net/Uri;",
            &[(&url).into()],
        )?
        .l()?;
    let action = env.new_string("android.intent.action.VIEW")?;
    let intent = env.new_object(
        "android/content/Intent",
        "(Ljava/lang/String;Landroid/net/Uri;)V",
        &[(&action).into(), (&uri).into()],
    )?;
    let category = env.new_string("android.intent.category.BROWSABLE")?;
    env.call_method(
        &intent,
        "addCategory",
        "(Ljava/lang/String;)Landroid/content/Intent;",
        &[(&category).into()],
    )?;
    env.call_method(
        &intent,
        "addFlags",
        "(I)Landroid/content/Intent;",
        &[FLAG_ACTIVITY_NEW_TASK.into()],
    )?;
    env.call_method(
        context,
        "startActivity",
        "(Landroid/content/Intent;)V",
        &[(&intent).into()],
    )?;
    Ok(())
}

/// Opens the Storage Access Framework picker over every document and blocks until the user
/// chooses or cancels; the caller checks filename extensions.
///
/// Returns private copies of the chosen documents, an empty list on cancellation, `None` when no
/// activity or document provider could show the picker, or an error when a document exceeds
/// `max_bytes` or could not be copied.
pub(super) fn pick_documents(
    multiple: bool,
    max_bytes: Option<u64>,
) -> Result<Option<Vec<PathBuf>>, String> {
    let context = ndk_context::android_context();
    // The Android runtime owns the VM and the application context for the process lifetime.
    let vm = unsafe { jni::JavaVM::from_raw(context.vm().cast()) }.map_err(|e| e.to_string())?;
    let mut env = vm.attach_current_thread().map_err(|e| e.to_string())?;
    let application = unsafe { JObject::from_raw(context.context().cast()) };
    // Java has no unsigned long; a negative limit means unlimited.
    let max_bytes = max_bytes.map_or(-1, |bytes| i64::try_from(bytes).unwrap_or(i64::MAX));
    let result = call_picker(&mut env, &application, multiple, max_bytes);
    result.map_err(|error| pending_exception(&mut env).unwrap_or_else(|| error.to_string()))
}

/// Calls `FilePicker.pick`, loading the class through the app's loader because worker threads
/// attached from native code only see system classes.
fn call_picker(
    env: &mut JNIEnv,
    application: &JObject,
    multiple: bool,
    max_bytes: i64,
) -> jni::errors::Result<Option<Vec<PathBuf>>> {
    let package = env
        .call_method(application, "getPackageName", "()Ljava/lang/String;", &[])?
        .l()?;
    let package: String = env.get_string(&JString::from(package))?.into();
    let loader = env
        .call_method(
            application,
            "getClassLoader",
            "()Ljava/lang/ClassLoader;",
            &[],
        )?
        .l()?;
    let name = env.new_string(format!("{package}.FilePicker"))?;
    let class = env
        .call_method(
            &loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[(&name).into()],
        )?
        .l()?;
    let paths = env
        .call_static_method(
            JClass::from(class),
            "pick",
            "(ZJ)[Ljava/lang/String;",
            &[multiple.into(), max_bytes.into()],
        )?
        .l()?;
    if paths.is_null() {
        return Ok(None);
    }
    let paths = JObjectArray::from(paths);
    let count = env.get_array_length(&paths)?;
    let mut picked = Vec::with_capacity(count.max(0) as usize);
    for index in 0..count {
        let path = JString::from(env.get_object_array_element(&paths, index)?);
        let path: String = env.get_string(&path)?.into();
        picked.push(PathBuf::from(path));
    }
    Ok(Some(picked))
}

/// The Java package holding the app's classes: `application_id` in the packaging manifest. A
/// build can install under another package name, so the installed name does not locate them.
fn java_package() -> String {
    #[derive(serde::Deserialize)]
    struct Runtime {
        application_id: String,
    }
    let runtime: Runtime =
        serde_json::from_str(include_str!("../../../../packaging/android/runtime.json"))
            .expect("the tracked Android runtime manifest parses");
    runtime.application_id
}

/// Whether `CrashReport.canSaveSession` says this device can save a report of the running
/// session; false when the question itself fails.
pub(super) fn saves_diagnostic_reports() -> bool {
    with_report_class(|env, class, _| {
        env.call_static_method(class, "canSaveSession", "()Z", &[])?
            .z()
    })
    .unwrap_or_else(|error| {
        tracing::warn!("the report exporter could not be reached: {error}");
        false
    })
}

/// Asks `CrashReport.saveSession` to write a report of the running session to Downloads. The
/// Java side writes it on its own worker and shows the result, so this returns at once.
pub(super) fn save_diagnostic_report() -> Result<(), String> {
    with_report_class(|env, class, application| {
        env.call_static_method(
            class,
            "saveSession",
            "(Landroid/content/Context;)V",
            &[application.into()],
        )?;
        Ok(())
    })
}

/// Runs `call` with the app's `CrashReport` class and the application context. The class is
/// loaded through the app's loader because threads attached from native code only see system
/// classes. A Java exception is cleared and returned as its message.
fn with_report_class<T>(
    call: impl FnOnce(&mut JNIEnv, JClass, &JObject) -> jni::errors::Result<T>,
) -> Result<T, String> {
    let context = ndk_context::android_context();
    // The Android runtime owns the VM and the application context for the process lifetime.
    let vm = unsafe { jni::JavaVM::from_raw(context.vm().cast()) }.map_err(|e| e.to_string())?;
    let mut env = vm.attach_current_thread().map_err(|e| e.to_string())?;
    let application = unsafe { JObject::from_raw(context.context().cast()) };
    let result =
        report_class(&mut env, &application).and_then(|class| call(&mut env, class, &application));
    result.map_err(|error| pending_exception(&mut env).unwrap_or_else(|| error.to_string()))
}

/// Loads the app's `CrashReport` class through the application's class loader.
fn report_class<'local>(
    env: &mut JNIEnv<'local>,
    application: &JObject,
) -> jni::errors::Result<JClass<'local>> {
    let loader = env
        .call_method(
            application,
            "getClassLoader",
            "()Ljava/lang/ClassLoader;",
            &[],
        )?
        .l()?;
    let name = env.new_string(format!("{}.CrashReport", java_package()))?;
    let class = env
        .call_method(
            &loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[(&name).into()],
        )?
        .l()?;
    Ok(JClass::from(class))
}

/// Clears a pending Java exception and returns its message, or its class when it has none, for
/// the import status line.
fn pending_exception(env: &mut JNIEnv) -> Option<String> {
    let throwable = env.exception_occurred().ok()?;
    if throwable.is_null() {
        return None;
    }
    env.exception_clear().ok()?;
    for method in ["getMessage", "toString"] {
        let message = env
            .call_method(&throwable, method, "()Ljava/lang/String;", &[])
            .and_then(|value| value.l())
            .ok()?;
        if !message.is_null() {
            return Some(env.get_string(&JString::from(message)).ok()?.into());
        }
    }
    None
}
