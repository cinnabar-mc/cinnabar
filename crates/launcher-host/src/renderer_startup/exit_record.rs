//! The platform's own record of why this app's previous process exited.
//!
//! Android 11 and later keep one per exit (`ApplicationExitInfo`). Other platforms keep none,
//! so their runs are judged from the run marker alone.

/// One exit as Android reports it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExitRecord {
    /// `ApplicationExitInfo.getReason()`.
    pub reason: i32,
    /// `ApplicationExitInfo.getStatus()`: an exit code or a signal number, by reason.
    pub status: i32,
    /// When the process died, in milliseconds since the Unix epoch.
    pub timestamp_ms: u64,
}

/// What an exit record says about a run that left no cause of its own.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExitKind {
    /// No record covers the run.
    Unknown,
    /// A fault signal in native code.
    NativeCrash,
    /// Anything else: an exit the app chose, a kill by the player or the system, low memory,
    /// an unresponsive app or a Java exception.
    Other,
}

/// `ApplicationExitInfo.REASON_SIGNALED`.
const REASON_SIGNALED: i32 = 2;
/// `ApplicationExitInfo.REASON_CRASH_NATIVE`.
const REASON_CRASH_NATIVE: i32 = 5;
/// Signals a fault raises: `SIGILL`, `SIGTRAP`, `SIGABRT`, `SIGBUS`, `SIGFPE`, `SIGSEGV` and
/// `SIGSYS`, as numbered on Linux for every architecture Android runs on.
const FAULT_SIGNALS: [i32; 7] = [4, 5, 6, 7, 8, 11, 31];

impl ExitRecord {
    /// Classifies this exit. Android reports a native crash by reason, or on some releases as
    /// a plain signal death.
    pub fn kind(self) -> ExitKind {
        match self.reason {
            REASON_CRASH_NATIVE => ExitKind::NativeCrash,
            REASON_SIGNALED if FAULT_SIGNALS.contains(&self.status) => ExitKind::NativeCrash,
            _ => ExitKind::Other,
        }
    }

    /// Classifies `exit` for a run that began at `started_ms`. A record from before the run
    /// began belongs to an older process, so the run itself left none.
    pub fn kind_for(exit: Option<Self>, started_ms: u64) -> ExitKind {
        match exit {
            Some(exit) if exit.timestamp_ms >= started_ms => exit.kind(),
            _ => ExitKind::Unknown,
        }
    }
}

/// The latest exit Android recorded for this app, or `None` before Android 11, when nothing
/// is recorded or when the query fails.
#[cfg(target_os = "android")]
pub fn previous() -> Option<ExitRecord> {
    match android::previous() {
        Ok(record) => record,
        Err(error) => {
            tracing::warn!(%error, "could not read the previous process exit");
            None
        }
    }
}

/// No exit records exist off Android.
#[cfg(not(target_os = "android"))]
pub fn previous() -> Option<ExitRecord> {
    None
}

#[cfg(target_os = "android")]
mod android {
    use super::ExitRecord;
    use jni::{
        JNIEnv,
        objects::{JObject, JValue},
    };

    /// The first Android release with `ActivityManager.getHistoricalProcessExitReasons`.
    const EXIT_REASONS_SDK: i32 = 30;

    /// Asks the activity manager for this package's most recent exit.
    pub(super) fn previous() -> jni::errors::Result<Option<ExitRecord>> {
        let context = ndk_context::android_context();
        // The Android runtime owns the VM and the application context for the process lifetime.
        let vm = unsafe { jni::JavaVM::from_raw(context.vm().cast())? };
        let mut env = vm.attach_current_thread()?;
        let application = unsafe { JObject::from_raw(context.context().cast()) };
        let result = latest(&mut env, &application);
        if result.is_err() && env.exception_check()? {
            env.exception_clear()?;
        }
        result
    }

    /// Reads reason, status and time of the newest record, as the startup dialog's Java does.
    fn latest(env: &mut JNIEnv, context: &JObject) -> jni::errors::Result<Option<ExitRecord>> {
        let sdk = env
            .get_static_field("android/os/Build$VERSION", "SDK_INT", "I")?
            .i()?;
        if sdk < EXIT_REASONS_SDK {
            return Ok(None);
        }
        let service = env.new_string("activity")?;
        let manager = env
            .call_method(
                context,
                "getSystemService",
                "(Ljava/lang/String;)Ljava/lang/Object;",
                &[(&service).into()],
            )?
            .l()?;
        let package = env
            .call_method(context, "getPackageName", "()Ljava/lang/String;", &[])?
            .l()?;
        let exits = env
            .call_method(
                &manager,
                "getHistoricalProcessExitReasons",
                "(Ljava/lang/String;II)Ljava/util/List;",
                &[(&package).into(), JValue::Int(0), JValue::Int(1)],
            )?
            .l()?;
        if env.call_method(&exits, "isEmpty", "()Z", &[])?.z()? {
            return Ok(None);
        }
        let exit = env
            .call_method(&exits, "get", "(I)Ljava/lang/Object;", &[JValue::Int(0)])?
            .l()?;
        let reason = env.call_method(&exit, "getReason", "()I", &[])?.i()?;
        let status = env.call_method(&exit, "getStatus", "()I", &[])?.i()?;
        let timestamp = env.call_method(&exit, "getTimestamp", "()J", &[])?.j()?;
        Ok(Some(ExitRecord {
            reason,
            status,
            timestamp_ms: u64::try_from(timestamp).unwrap_or(0),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Android's reason numbers for exits that are not native faults: an exit the app chose,
    /// low memory, a Java crash, an unresponsive app, and requests by the player.
    const OTHER_REASONS: [i32; 7] = [0, 1, 3, 4, 6, 10, 11];

    #[test]
    fn only_native_faults_count_as_native_crashes() {
        for status in [0, 6, 11] {
            let exit = ExitRecord {
                reason: REASON_CRASH_NATIVE,
                status,
                timestamp_ms: 0,
            };
            assert_eq!(exit.kind(), ExitKind::NativeCrash);
        }
        for signal in FAULT_SIGNALS {
            let exit = ExitRecord {
                reason: REASON_SIGNALED,
                status: signal,
                timestamp_ms: 0,
            };
            assert_eq!(exit.kind(), ExitKind::NativeCrash, "signal {signal}");
        }
        // SIGKILL and SIGTERM are how the system and the player stop an app.
        for signal in [9, 15] {
            let exit = ExitRecord {
                reason: REASON_SIGNALED,
                status: signal,
                timestamp_ms: 0,
            };
            assert_eq!(exit.kind(), ExitKind::Other, "signal {signal}");
        }
        for reason in OTHER_REASONS {
            for status in [0, 1, 11] {
                let exit = ExitRecord {
                    reason,
                    status,
                    timestamp_ms: 0,
                };
                assert_eq!(exit.kind(), ExitKind::Other, "reason {reason}");
            }
        }
    }

    #[test]
    fn a_record_older_than_the_run_says_nothing_about_it() {
        let crash = ExitRecord {
            reason: REASON_CRASH_NATIVE,
            status: 11,
            timestamp_ms: 5_000,
        };
        assert_eq!(
            ExitRecord::kind_for(Some(crash), 4_000),
            ExitKind::NativeCrash
        );
        assert_eq!(
            ExitRecord::kind_for(Some(crash), 5_000),
            ExitKind::NativeCrash
        );
        assert_eq!(ExitRecord::kind_for(Some(crash), 5_001), ExitKind::Unknown);
        assert_eq!(ExitRecord::kind_for(None, 0), ExitKind::Unknown);
    }
}
