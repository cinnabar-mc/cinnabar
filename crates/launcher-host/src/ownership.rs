//! The ownership gate's proof sources: Minecraft installed from this platform's store, and the
//! license account, checked by a one-shot `bedrock-core -auth-events -ownership` helper.

#[cfg(target_os = "android")]
pub mod android;

use std::{
    process::Command,
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crossbeam_channel::{RecvTimeoutError, bounded};
use launcher::{
    install_layout::InstallLayout,
    ownership::{self as policy, Check, LocalProbe, Store, Verdict},
};

use crate::{
    auth::{EVENT_CAPACITY, ReaderMessage, WireEvent, read_events, valid_code, valid_https_uri},
    lifecycle::{
        children::Spawned,
        core_output::{self, CoreRole},
    },
};

/// The Windows package family of Minecraft for Windows, from the Microsoft Store or Xbox app.
pub const MINECRAFT_PACKAGE_FAMILY: &str = "Microsoft.MinecraftUWP_8wekyb3d8bbwe";

/// How often a running check looks for cancellation.
const CANCEL_POLL: Duration = Duration::from_millis(100);

/// The store [`probe_local`] looks for on this platform.
#[cfg(windows)]
const PROBED_STORE: Option<Store> = Some(Store::Microsoft);
#[cfg(target_os = "android")]
const PROBED_STORE: Option<Store> = Some(Store::Android);
#[cfg(not(any(windows, target_os = "android")))]
const PROBED_STORE: Option<Store> = None;

const _: () = assert!(
    Store::same(PROBED_STORE, policy::LOCAL_STORE),
    "every local store needs a probe here, and every probe needs its store in the policy"
);

/// The Windows package family of Gaming Services, which the Store needs to install the game.
pub const GAMING_SERVICES_PACKAGE_FAMILY: &str = "Microsoft.GamingServices_8wekyb3d8bbwe";

/// One look at this device's store install, uncached; every gate decision of a check comes from
/// it. Best-effort: package metadata shows the game is installed, not that its license is current.
#[cfg(windows)]
#[must_use]
pub fn probe_local() -> LocalProbe {
    let found = registered(MINECRAFT_PACKAGE_FAMILY);
    let atlas_os = !found && atlas_os();
    LocalProbe::windows(
        found,
        atlas_os,
        atlas_os && registered(GAMING_SERVICES_PACKAGE_FAMILY),
    )
}

/// Whether a package of `family` is registered for the current user.
#[cfg(windows)]
fn registered(family: &str) -> bool {
    use windows_sys::Win32::{
        Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS},
        Storage::Packaging::Appx::GetPackagesByPackageFamily,
    };
    let family: Vec<u16> = family.encode_utf16().chain(Some(0)).collect();
    let (mut count, mut length) = (0u32, 0u32);
    // SAFETY: the family name is NUL-terminated; null buffers with zero lengths ask only for counts.
    let status = unsafe {
        GetPackagesByPackageFamily(
            family.as_ptr(),
            &mut count,
            std::ptr::null_mut(),
            &mut length,
            std::ptr::null_mut(),
        )
    };
    matches!(status, ERROR_SUCCESS | ERROR_INSUFFICIENT_BUFFER) && count > 0
}

/// Whether this Windows carries the markers AtlasOS's playbook writes.
#[cfg(windows)]
fn atlas_os() -> bool {
    use windows_sys::Win32::{
        Foundation::ERROR_SUCCESS,
        System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW},
    };
    let wide = |text: &str| text.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    [
        (
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\OEMInformation",
            "Model",
        ),
        (
            r"SOFTWARE\Microsoft\Windows NT\CurrentVersion",
            "RegisteredOrganization",
        ),
    ]
    .into_iter()
    .any(|(key, name)| {
        let (key, name) = (wide(key), wide(name));
        let mut value = [0u16; 128];
        let mut bytes = std::mem::size_of_val(&value) as u32;
        // SAFETY: key and name are NUL-terminated, and `bytes` is `value`'s size in bytes.
        let status = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                value.as_mut_ptr().cast(),
                &mut bytes,
            )
        };
        let len = value
            .iter()
            .position(|&unit| unit == 0)
            .unwrap_or(value.len());
        status == ERROR_SUCCESS
            && policy::is_atlas_os_marker(&String::from_utf16_lossy(&value[..len]))
    })
}

/// One look at this device's Google Play and, where Play keeps the source, its Minecraft install.
#[cfg(target_os = "android")]
#[must_use]
pub fn probe_local() -> LocalProbe {
    android::probe()
}

/// No store install can prove ownership on this platform.
#[cfg(not(any(windows, target_os = "android")))]
#[must_use]
pub const fn probe_local() -> LocalProbe {
    LocalProbe {
        store_usable: true,
        install: policy::LocalInstall::NotFound,
        store_app: None,
    }
}

/// One account check's answer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Finding {
    pub check: Check,
    /// A short credential-free code for the log.
    pub reason: String,
    /// The stored license sign-in the answer is about; empty when none is stored.
    pub account: String,
}

impl Finding {
    fn unverifiable(reason: &str) -> Self {
        Self {
            check: Check::Unverifiable,
            reason: reason.to_owned(),
            account: String::new(),
        }
    }
}

/// Whether this install's saved pass lets a launch play now and check in the background.
#[must_use]
pub fn pass_valid(layout: &InstallLayout) -> bool {
    policy::pass_valid(
        policy::load_pass(&layout.ownership_pass_file()).as_ref(),
        unix_now(),
    )
}

/// Runs [`check_account`] and records its answer in the pass, holding the check lock throughout.
/// A cancelled check records nothing and is unverifiable.
pub fn check_and_record(
    layout: &InstallLayout,
    client_id: &str,
    sign_in: bool,
    cancel: &AtomicBool,
    on_code: impl FnMut(&str, &str),
) -> Verdict {
    let license = layout.license_cache();
    let lock = match policy::lock_checks(&license) {
        Ok(lock) => lock,
        Err(error) => {
            tracing::warn!(kind = ?error.kind(), "ownership check not started");
            return Verdict::Deny(Check::Unverifiable);
        }
    };
    let finding = check_account(layout, client_id, sign_in, cancel, on_code);
    if cancel.load(Ordering::Relaxed) {
        return Verdict::Deny(Check::Unverifiable);
    }
    let (verdict, saved) = policy::record_check(
        &lock,
        &license,
        &layout.ownership_pass_file(),
        finding.check,
        &finding.account,
        unix_now(),
    );
    if let Err(error) = saved {
        tracing::warn!(kind = ?error.kind(), "ownership pass not saved");
    }
    tracing::info!(
        source = "account",
        result = ?finding.check,
        reason = %finding.reason,
        allowed = verdict == Verdict::Allow,
        "ownership checked"
    );
    verdict
}

/// Checks the license account with `client_id`, Cinnabar's app registration. `sign_in` replaces
/// the stored account through a device code, which `on_code` shows. Blocks until the helper
/// answers or `cancel` is set; every failure is unverifiable.
pub fn check_account(
    layout: &InstallLayout,
    client_id: &str,
    sign_in: bool,
    cancel: &AtomicBool,
    on_code: impl FnMut(&str, &str),
) -> Finding {
    let Some(executable) = crate::core_process::core_executable(layout) else {
        return Finding::unverifiable("core_missing");
    };
    let mut command = Command::new(executable);
    command
        .arg("-auth-events")
        .arg("-ownership")
        .arg("-auth-cache")
        .arg(layout.license_cache())
        .arg("-ownership-client-id")
        .arg(client_id);
    if sign_in {
        command.arg("-ownership-sign-in");
    }
    match core_output::spawn(&mut command, CoreRole::SignIn) {
        Ok(child) => follow(child, cancel, on_code),
        Err(_) => Finding::unverifiable("core_start"),
    }
}

/// Reads `child`'s events until its answer, then reaps it.
fn follow(child: Spawned, cancel: &AtomicBool, mut on_code: impl FnMut(&str, &str)) -> Finding {
    let Some(stdout) = child.take_stdout() else {
        child.kill();
        return Finding::unverifiable("core_stdout");
    };
    let (sender, receiver) = bounded(EVENT_CAPACITY);
    let reader = thread::spawn(move || read_events(stdout, sender));
    let finding = loop {
        if cancel.load(Ordering::Relaxed) {
            break Finding::unverifiable("cancelled");
        }
        let line = match receiver.recv_timeout(CANCEL_POLL) {
            Ok(ReaderMessage::Line(line)) => line,
            Err(RecvTimeoutError::Timeout) => continue,
            Ok(ReaderMessage::Invalid(_)) => {
                break Finding::unverifiable("helper_output");
            }
            Ok(ReaderMessage::End) | Err(RecvTimeoutError::Disconnected) => {
                break Finding::unverifiable("helper_exited");
            }
        };
        match serde_json::from_slice::<WireEvent>(&line) {
            Ok(event) if event.version() != 1 => break Finding::unverifiable("helper_output"),
            Ok(WireEvent::CheckingCache { .. }) => {}
            Ok(WireEvent::DeviceCode {
                verification_uri,
                user_code,
                ..
            }) if valid_https_uri(&verification_uri) && valid_code(&user_code) => {
                on_code(&verification_uri, &user_code);
            }
            Ok(WireEvent::Ownership {
                result,
                reason,
                account,
                ..
            }) => {
                break Finding {
                    check: result,
                    reason: log_safe(&reason),
                    account: log_safe(&account),
                };
            }
            _ => break Finding::unverifiable("helper_output"),
        }
    };
    // The helper saves everything before it answers, so nothing is lost by stopping it now.
    child.kill();
    let _ = child.wait();
    let _ = reader.join();
    finding
}

/// Keeps a helper's reason code or sign-in generation to a short, printable token.
fn log_safe(reason: &str) -> String {
    reason
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | ':' | '-'))
        .take(64)
        .collect()
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

#[cfg(test)]
mod tests;
