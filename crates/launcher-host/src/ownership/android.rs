//! Android's local-install probe: `PackageManager` reads for the decision in
//! [`launcher::ownership::android`].

use std::sync::{Once, OnceLock};

use jni::{
    JNIEnv, JavaVM,
    objects::{GlobalRef, JByteArray, JObject, JObjectArray, JString},
};
use launcher::ownership::{
    LocalProbe,
    android::{self as policy, InstallSource, Packages, ReadFailed},
};
use sha2::{Digest, Sha256};

/// `PackageManager.CERT_INPUT_SHA256`.
const CERT_INPUT_SHA256: i32 = 1;
/// `PackageManager.GET_SIGNATURES`.
const GET_SIGNATURES: i32 = 0x40;
/// `PackageManager.MATCH_DISABLED_UNTIL_USED_COMPONENTS`, so a package disabled until used still
/// reads as installed.
const MATCH_DISABLED_UNTIL_USED: i32 = 0x8000;
/// `PackageManager.COMPONENT_ENABLED_STATE_DISABLED`, `_DISABLED_USER` and
/// `_DISABLED_UNTIL_USED`.
const DISABLED_STATES: [i32; 3] = [2, 3, 4];
/// Local references one probe creates, with room to spare.
const LOCAL_FRAME: i32 = 32;

/// The application context, for processes whose activity is not the `NativeActivity`.
static CONTEXT: OnceLock<(JavaVM, GlobalRef)> = OnceLock::new();

/// Lets [`probe`] run where `ndk_context` is not set, such as the setup activity; later calls
/// keep the first context.
pub fn use_context(env: &mut JNIEnv<'_>, context: &JObject<'_>) -> jni::errors::Result<()> {
    if CONTEXT.get().is_some() {
        return Ok(());
    }
    let application = env
        .call_method(
            context,
            "getApplicationContext",
            "()Landroid/content/Context;",
            &[],
        )?
        .l()?;
    let global = env.new_global_ref(application)?;
    let _ = CONTEXT.set((env.get_java_vm()?, global));
    Ok(())
}

/// Probes Play and the game once on the calling thread, uncached, logging the result, and the
/// first time Play's state removes the source, why. A probe that cannot reach `PackageManager`
/// at all is unreadable too.
pub(super) fn probe() -> LocalProbe {
    static UNAVAILABLE_LOGGED: Once = Once::new();
    // A closure, since the generic policy function names no single `Jni` lifetime.
    let probe = with_packages(|packages: &mut Jni<'_, '_>| policy::probe(packages)).unwrap_or_else(
        |error| {
            tracing::warn!(%error, "ownership probe failed");
            policy::Probe {
                play: policy::PlayState::Unreadable,
                install: Some(policy::Install::Unreadable),
            }
        },
    );
    if !probe.play.keeps_source() {
        UNAVAILABLE_LOGGED.call_once(|| {
            tracing::info!(
                source = "local_install",
                play = probe.play.reason(),
                "ownership source unavailable; gate off on this device"
            );
        });
    }
    if let Some(install) = probe.install {
        tracing::info!(
            source = "local_install",
            play = probe.play.reason(),
            result = install.reason(),
            "ownership probed"
        );
    }
    probe.local()
}

/// Runs `read` over this app's `PackageManager`.
fn with_packages<T>(read: impl FnOnce(&mut Jni<'_, '_>) -> T) -> jni::errors::Result<T> {
    with_context(|env, context| {
        env.with_local_frame(LOCAL_FRAME, |env| -> jni::errors::Result<_> {
            let manager = env
                .call_method(
                    context,
                    "getPackageManager",
                    "()Landroid/content/pm/PackageManager;",
                    &[],
                )?
                .l()?;
            Ok(read(&mut Jni { env, manager }))
        })
    })
}

/// Runs `probe` with an attached environment and the application context.
fn with_context<T>(
    probe: impl FnOnce(&mut JNIEnv<'_>, &JObject<'_>) -> jni::errors::Result<T>,
) -> jni::errors::Result<T> {
    if let Some((vm, context)) = CONTEXT.get() {
        let mut env = vm.attach_current_thread()?;
        return clearing(&mut *env, |env| probe(env, context.as_obj()));
    }
    let context = ndk_context::android_context();
    // SAFETY: android-activity publishes the process's live VM and Application here.
    let vm = unsafe { JavaVM::from_raw(context.vm().cast())? };
    let mut env = vm.attach_current_thread()?;
    let application = unsafe { JObject::from_raw(context.context().cast()) };
    clearing(&mut *env, |env| probe(env, &application))
}

/// Clears any pending Java exception; true when it was `NameNotFoundException`, which means the
/// package is not installed for this user.
fn clear_absent_package(env: &mut JNIEnv<'_>) -> bool {
    let thrown = env
        .exception_occurred()
        .ok()
        .filter(|thrown| !thrown.is_null());
    let _ = env.exception_clear();
    thrown.is_some_and(|thrown| {
        env.is_instance_of(
            &thrown,
            "android/content/pm/PackageManager$NameNotFoundException",
        )
        .unwrap_or(false)
    })
}

/// Runs `call`, clearing any Java exception it left pending.
fn clearing<'local, T>(
    env: &mut JNIEnv<'local>,
    call: impl FnOnce(&mut JNIEnv<'local>) -> jni::errors::Result<T>,
) -> jni::errors::Result<T> {
    let result = call(env);
    if result.is_err() && env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }
    result
}

/// [`Packages`] over a live `PackageManager`.
struct Jni<'a, 'local> {
    env: &'a mut JNIEnv<'local>,
    manager: JObject<'local>,
}

impl<'local> Jni<'_, 'local> {
    /// One read; any failure or thrown exception is [`ReadFailed`].
    fn read<T>(
        &mut self,
        read: impl FnOnce(&mut JNIEnv<'local>, &JObject<'local>) -> jni::errors::Result<T>,
    ) -> Result<T, ReadFailed> {
        let Self { env, manager } = self;
        clearing(&mut **env, |env| read(env, &*manager)).map_err(|_| ReadFailed)
    }
}

impl Packages for Jni<'_, '_> {
    fn sdk(&mut self) -> Result<u32, ReadFailed> {
        let sdk = self.read(|env, _| {
            env.get_static_field("android/os/Build$VERSION", "SDK_INT", "I")?
                .i()
        })?;
        u32::try_from(sdk).map_err(|_| ReadFailed)
    }

    fn installed(&mut self, package: &str) -> Result<bool, ReadFailed> {
        let Self { env, manager } = self;
        match package_info(&mut **env, &*manager, package, MATCH_DISABLED_UNTIL_USED) {
            Ok(info) => Ok(!info.is_null()),
            // Only `NameNotFoundException` means absent; anything else is a failed read.
            Err(_) if clear_absent_package(&mut **env) => Ok(false),
            Err(_) => Err(ReadFailed),
        }
    }

    fn enabled(&mut self, package: &str) -> Result<bool, ReadFailed> {
        self.read(|env, manager| {
            let name = env.new_string(package)?;
            let info = env
                .call_method(
                    manager,
                    "getApplicationInfo",
                    "(Ljava/lang/String;I)Landroid/content/pm/ApplicationInfo;",
                    &[(&name).into(), MATCH_DISABLED_UNTIL_USED.into()],
                )?
                .l()?;
            let enabled = env.get_field(&info, "enabled", "Z")?.z()?;
            let setting = env
                .call_method(
                    manager,
                    "getApplicationEnabledSetting",
                    "(Ljava/lang/String;)I",
                    &[(&name).into()],
                )?
                .i()?;
            Ok(enabled && !DISABLED_STATES.contains(&setting))
        })
    }

    fn has_signing_certificate(
        &mut self,
        package: &str,
        sha256: &[u8; 32],
    ) -> Result<bool, ReadFailed> {
        self.read(|env, manager| {
            let name = env.new_string(package)?;
            let digest = env.byte_array_from_slice(sha256)?;
            env.call_method(
                manager,
                "hasSigningCertificate",
                "(Ljava/lang/String;[BI)Z",
                &[(&name).into(), (&digest).into(), CERT_INPUT_SHA256.into()],
            )?
            .z()
        })
    }

    fn signature_digests(&mut self, package: &str) -> Result<Vec<[u8; 32]>, ReadFailed> {
        self.read(|env, manager| {
            let info = package_info(
                env,
                manager,
                package,
                GET_SIGNATURES | MATCH_DISABLED_UNTIL_USED,
            )?;
            let signatures = env
                .get_field(&info, "signatures", "[Landroid/content/pm/Signature;")?
                .l()?;
            if signatures.is_null() {
                return Ok(Vec::new());
            }
            let signatures = JObjectArray::from(signatures);
            let mut digests = Vec::new();
            for index in 0..env.get_array_length(&signatures)? {
                let signature = env.get_object_array_element(&signatures, index)?;
                let bytes = env
                    .call_method(&signature, "toByteArray", "()[B", &[])?
                    .l()?;
                let bytes = env.convert_byte_array(JByteArray::from(bytes))?;
                digests.push(Sha256::digest(&bytes).into());
            }
            Ok(digests)
        })
    }

    fn install_source(&mut self, package: &str) -> Result<InstallSource, ReadFailed> {
        self.read(|env, manager| {
            let name = env.new_string(package)?;
            let info = env
                .call_method(
                    manager,
                    "getInstallSourceInfo",
                    "(Ljava/lang/String;)Landroid/content/pm/InstallSourceInfo;",
                    &[(&name).into()],
                )?
                .l()?;
            Ok(InstallSource {
                installing: string_call(env, &info, "getInstallingPackageName")?,
                initiating: string_call(env, &info, "getInitiatingPackageName")?,
            })
        })
    }

    fn installer(&mut self, package: &str) -> Result<Option<String>, ReadFailed> {
        self.read(|env, manager| {
            let name = env.new_string(package)?;
            let installer = env
                .call_method(
                    manager,
                    "getInstallerPackageName",
                    "(Ljava/lang/String;)Ljava/lang/String;",
                    &[(&name).into()],
                )?
                .l()?;
            optional_string(env, installer)
        })
    }
}

/// `getPackageInfo(package, flags)`; throws `NameNotFoundException` when not installed for this
/// user.
fn package_info<'local>(
    env: &mut JNIEnv<'local>,
    manager: &JObject<'_>,
    package: &str,
    flags: i32,
) -> jni::errors::Result<JObject<'local>> {
    let name = env.new_string(package)?;
    env.call_method(
        manager,
        "getPackageInfo",
        "(Ljava/lang/String;I)Landroid/content/pm/PackageInfo;",
        &[(&name).into(), flags.into()],
    )?
    .l()
}

/// Calls a no-argument `String` getter on `info`.
fn string_call(
    env: &mut JNIEnv<'_>,
    info: &JObject<'_>,
    getter: &str,
) -> jni::errors::Result<Option<String>> {
    let value = env
        .call_method(info, getter, "()Ljava/lang/String;", &[])?
        .l()?;
    optional_string(env, value)
}

/// A Java string that may be null.
fn optional_string(
    env: &mut JNIEnv<'_>,
    value: JObject<'_>,
) -> jni::errors::Result<Option<String>> {
    if value.is_null() {
        return Ok(None);
    }
    Ok(Some(env.get_string(&JString::from(value))?.into()))
}
