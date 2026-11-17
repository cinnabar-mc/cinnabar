//! Whether this install may start: proof that the player owns Minecraft, from any source the
//! platform offers, and the cached pass that keeps account proof usable offline.

use std::{io, path::Path};

use serde::{Deserialize, Serialize};

/// Cinnabar's own Microsoft app registration, set at build time from `CINNABAR_MSA_CLIENT_ID`.
/// Without one the account source is unavailable.
pub const MSA_CLIENT_ID: Option<&str> = match option_env!("CINNABAR_MSA_CLIENT_ID") {
    Some(id) if !id.is_empty() => Some(id),
    _ => None,
};

const _: () = assert!(
    match MSA_CLIENT_ID {
        Some(id) => valid_client_id(id),
        None => true,
    },
    "CINNABAR_MSA_CLIENT_ID must be an application (client) ID GUID"
);

/// Where the failure screen sends a player who doesn't own Minecraft.
pub const GET_MINECRAFT_URL: &str = "https://www.minecraft.net/";

/// How long a successful account check lets the game start without another.
pub const PASS_LIFETIME_SECS: u64 = 30 * 24 * 60 * 60;
/// Two not-owned answers this far apart revoke a pass; one alone may be a service glitch.
pub const REVOKE_AFTER_SECS: u64 = 24 * 60 * 60;
/// How far the clock may step back (time sync, time zones) before it forces a check.
const CLOCK_SLACK_SECS: u64 = 10 * 60;

/// The proof sources this build offers on this platform.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Sources {
    /// The game installed from its store on this device; best-effort.
    pub local_install: bool,
    /// A Microsoft account signed in through Cinnabar's app.
    pub account: bool,
}

impl Sources {
    /// This build's sources, given whether the platform can look for a local install.
    #[must_use]
    pub const fn for_build(local_install: bool) -> Self {
        Self {
            local_install,
            account: MSA_CLIENT_ID.is_some(),
        }
    }

    /// The gate applies only where some proof can pass it.
    #[must_use]
    pub const fn gated(self) -> bool {
        self.local_install || self.account
    }
}

/// This build's sources on this platform. Android and iOS start without the first-run gate, so
/// they offer none yet.
#[must_use]
pub const fn sources() -> Sources {
    if cfg!(any(target_os = "android", target_os = "ios")) {
        return Sources {
            local_install: false,
            account: false,
        };
    }
    Sources::for_build(cfg!(windows))
}

/// One account check's answer.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Check {
    Verified,
    /// The service answered and listed no Minecraft entitlement.
    NotOwned,
    /// No license account is stored, or its sign-in was rejected.
    SignInRequired,
    /// Anything else: offline, outages, refusals that say nothing about ownership.
    Unverifiable,
}

/// What a launch should do before the game starts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Start {
    /// A valid pass: play now and check again in the background.
    Play,
    /// No usable pass: check before playing.
    Verify,
}

/// Whether play may continue after a check.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Verdict {
    Allow,
    Deny(Check),
}

/// The license account's last check results; local evidence, not a signed proof.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Pass {
    /// Unix seconds of the last check that found Minecraft.
    pub verified_at: u64,
    /// Unix seconds of the first not-owned answer since then.
    #[serde(default)]
    pub not_owned_since: Option<u64>,
    /// The latest local time any check saw, so a clock set back is noticed.
    pub latest_clock: u64,
}

/// Whether `pass` lets a launch at `now` (unix seconds) play before checking.
#[must_use]
pub fn start(pass: Option<&Pass>, now: u64) -> Start {
    let Some(pass) = pass else {
        return Start::Verify;
    };
    let clock_back = now.saturating_add(CLOCK_SLACK_SECS) < pass.latest_clock;
    let expired = now < pass.verified_at
        || now - pass.verified_at >= PASS_LIFETIME_SECS
        || now.saturating_sub(pass.latest_clock) > PASS_LIFETIME_SECS;
    if clock_back || expired {
        Start::Verify
    } else {
        Start::Play
    }
}

/// The pass after `check` at `now`, and whether play may continue. `fresh_sign_in` means the
/// check used a newly signed-in account, so the old pass no longer applies. Only a verified
/// check extends a pass.
#[must_use]
pub fn record(
    pass: Option<Pass>,
    check: Check,
    fresh_sign_in: bool,
    now: u64,
) -> (Option<Pass>, Verdict) {
    let pass = pass.filter(|_| !fresh_sign_in).map(|pass| Pass {
        latest_clock: pass.latest_clock.max(now),
        ..pass
    });
    match (check, pass) {
        (Check::Verified, pass) => (
            Some(Pass {
                verified_at: now,
                not_owned_since: None,
                latest_clock: pass.map_or(now, |pass| pass.latest_clock),
            }),
            Verdict::Allow,
        ),
        (Check::NotOwned, Some(pass)) => match pass.not_owned_since {
            Some(since) if now >= since.saturating_add(REVOKE_AFTER_SECS) => {
                (None, Verdict::Deny(Check::NotOwned))
            }
            since => (
                Some(Pass {
                    not_owned_since: Some(since.map_or(now, |since| since.min(now))),
                    ..pass
                }),
                Verdict::Allow,
            ),
        },
        (Check::SignInRequired, Some(pass)) => {
            let verdict = if start(Some(&pass), now) == Start::Play {
                Verdict::Allow
            } else {
                Verdict::Deny(Check::SignInRequired)
            };
            (Some(pass), verdict)
        }
        (Check::Unverifiable, Some(pass)) => (Some(pass), Verdict::Allow),
        (check, None) => (None, Verdict::Deny(check)),
    }
}

/// Records `check` in the pass at `pass_path` while holding the license account's lock, so it
/// cannot race [`forget_license`]. A check whose account was removed meanwhile records nothing.
pub fn record_check(
    license_cache: &Path,
    pass_path: &Path,
    check: Check,
    fresh_sign_in: bool,
    now: u64,
) -> io::Result<Verdict> {
    let _leases = crate::accounts::cache_leases(license_cache)?;
    let (pass, verdict) = record(load_pass(pass_path), check, fresh_sign_in, now);
    if license_cache.is_file() {
        save_pass(pass_path, pass.as_ref())?;
    }
    Ok(verdict)
}

/// The saved pass; a missing or unreadable one is none.
#[must_use]
pub fn load_pass(path: &Path) -> Option<Pass> {
    let bytes = crate::accounts::read_optional(path).ok()??;
    serde_json::from_slice(&bytes).ok()
}

/// Saves `pass` privately, or removes the saved one.
pub fn save_pass(path: &Path, pass: Option<&Pass>) -> io::Result<()> {
    match pass {
        Some(pass) => crate::accounts::write_private(
            path,
            &serde_json::to_vec(pass).map_err(io::Error::other)?,
        ),
        None => crate::accounts::remove_optional(path),
    }
}

/// Forgets the license account: its credentials, the files the core keeps beside them, and
/// the pass. Waits for a check in progress to release the credentials.
pub fn forget_license(license_cache: &Path, pass: &Path) -> io::Result<()> {
    let _leases = crate::accounts::cache_leases(license_cache)?;
    let directory = license_cache.parent().unwrap_or(Path::new("."));
    let name = license_cache
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let file = entry.file_name().to_string_lossy().into_owned();
        // Lock files stay: other processes may hold them.
        if file.starts_with(&name) && !file.ends_with(".lock") {
            crate::accounts::remove_optional(&entry.path())?;
        }
    }
    save_pass(pass, None)
}

/// Whether `id` has the GUID shape of an application (client) ID.
const fn valid_client_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    let mut index = 0;
    while index < bytes.len() {
        let dash = matches!(index, 8 | 13 | 18 | 23);
        let ok = if dash {
            bytes[index] == b'-'
        } else {
            bytes[index].is_ascii_hexdigit()
        };
        if !ok {
            return false;
        }
        index += 1;
    }
    true
}

#[cfg(test)]
mod tests;
