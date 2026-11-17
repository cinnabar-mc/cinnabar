//! The ownership gate at the start of every packaged launch: play continues only when a proof
//! source this platform offers finds that the player owns Minecraft.

use std::{
    sync::atomic::AtomicBool,
    time::{SystemTime, UNIX_EPOCH},
};

use launcher::{
    install_layout::InstallLayout,
    ownership::{self as policy, Check, MSA_CLIENT_ID, Sources, Start, Verdict},
};
use launcher_host::{
    native_dialog::{Consent, Prompter},
    ownership as sources,
};

/// Why the gate stayed closed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Refusal {
    /// The local install is the only source and found no game.
    NotInstalled,
    NotOwned,
    SignInRequired,
    Unverifiable,
}

/// One blocking check's outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Answer {
    Allowed,
    Denied(Refusal),
}

/// Whether play may continue without asking the player: the gate is off, the local install was
/// found, or a valid pass exists (which is then re-checked in the background).
pub(crate) fn open_without_asking(layout: &InstallLayout) -> bool {
    let available = policy::sources();
    if !available.gated() {
        tracing::info!("ownership gate off: no proof source on this platform in this build");
        return true;
    }
    if available.local_install && sources::local_install_found() {
        tracing::info!(source = "local_install", "ownership verified");
        return true;
    }
    let Some(client_id) = MSA_CLIENT_ID.filter(|_| available.account) else {
        return false;
    };
    let pass = policy::load_pass(&layout.ownership_pass_file());
    if policy::start(pass.as_ref(), unix_now()) == Start::Verify {
        return false;
    }
    tracing::info!(source = "account_pass", "ownership verified");
    let layout = layout.clone();
    let _ = std::thread::Builder::new()
        .name("ownership-recheck".into())
        .spawn(move || {
            account(
                &layout,
                client_id,
                false,
                &AtomicBool::new(false),
                &mut |_, _| {},
            );
        });
    true
}

/// Checks every source once, the local install first. `sign_in` replaces the license account
/// through a device code, which `on_code` shows.
pub(crate) fn verify(
    layout: &InstallLayout,
    sign_in: bool,
    cancel: &AtomicBool,
    on_code: &mut dyn FnMut(&str, &str),
) -> Answer {
    let available = policy::sources();
    if available.local_install && !sign_in && sources::local_install_found() {
        tracing::info!(source = "local_install", "ownership verified");
        return Answer::Allowed;
    }
    let Some(client_id) = MSA_CLIENT_ID.filter(|_| available.account) else {
        tracing::info!(result = "not_installed", "ownership not verified");
        return Answer::Denied(Refusal::NotInstalled);
    };
    match account(layout, client_id, sign_in, cancel, on_code) {
        Verdict::Allow => Answer::Allowed,
        Verdict::Deny(check) => Answer::Denied(match check {
            Check::NotOwned => Refusal::NotOwned,
            Check::SignInRequired => Refusal::SignInRequired,
            Check::Verified | Check::Unverifiable => Refusal::Unverifiable,
        }),
    }
}

/// Checks the license account and records the answer in its pass.
fn account(
    layout: &InstallLayout,
    client_id: &str,
    sign_in: bool,
    cancel: &AtomicBool,
    on_code: &mut dyn FnMut(&str, &str),
) -> Verdict {
    let finding = sources::check_account(layout, client_id, sign_in, cancel, on_code);
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        return Verdict::Deny(Check::Unverifiable);
    }
    let verdict = policy::record_check(
        &layout.license_cache(),
        &layout.ownership_pass_file(),
        finding.check,
        finding.signed_in,
        unix_now(),
    )
    .unwrap_or_else(|error| {
        tracing::warn!(kind = ?error.kind(), "ownership pass not saved");
        policy::record(
            policy::load_pass(&layout.ownership_pass_file()),
            finding.check,
            finding.signed_in,
            unix_now(),
        )
        .1
    });
    tracing::info!(
        source = "account",
        result = ?finding.check,
        reason = %finding.reason,
        allowed = verdict == Verdict::Allow,
        "ownership checked"
    );
    verdict
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// Shown on every refusal.
pub(crate) const NOTICE: &str = "Cinnabar is not an official Minecraft product and is not approved by or associated with Mojang or Microsoft.";

/// The refusal's heading.
pub(crate) const fn title(refusal: Refusal) -> &'static str {
    match refusal {
        Refusal::NotInstalled => "Minecraft isn't installed",
        Refusal::NotOwned => "This account doesn't own Minecraft",
        Refusal::SignInRequired => "Confirm you own Minecraft",
        Refusal::Unverifiable => "Couldn't confirm ownership",
    }
}

/// The refusal's explanation for `available` sources, ending with the notice.
pub(crate) fn body(refusal: Refusal, available: Sources) -> String {
    let install = "On Windows, Cinnabar can use Minecraft installed from the Microsoft Store or the Xbox app. If you bought Minecraft for Windows or Minecraft: Java & Bedrock Edition for PC, it's already in your library there.";
    let mut text = match refusal {
        Refusal::NotInstalled => format!(
            "Cinnabar is for people who own Minecraft. {install} Install it, then select Try again."
        ),
        Refusal::NotOwned => "The Microsoft account you signed in with doesn't own Minecraft for PC. A copy bought on a phone or console doesn't include PC. Sign in with the account that bought it, or get Minecraft.".to_owned(),
        Refusal::SignInRequired => "Cinnabar is for people who own Minecraft. Sign in once with the Microsoft account that owns Minecraft for PC so Cinnabar can confirm it. Afterwards you can play with any of your accounts.".to_owned(),
        Refusal::Unverifiable => "Cinnabar couldn't reach Microsoft to confirm that you own Minecraft. Check your connection, then try again.".to_owned(),
    };
    if available.local_install && refusal != Refusal::NotInstalled {
        text.push('\n');
        text.push_str(install);
    }
    text.push('\n');
    text.push_str(NOTICE);
    text
}

/// The sign-in prompt's text.
pub(crate) fn code_prompt(uri: &str, code: &str) -> String {
    format!(
        "Open {uri} and enter the code {code}, then return here. Use the Microsoft account that owns Minecraft for PC."
    )
}

/// Runs the gate with native dialogs when no setup window can open; returns whether play may
/// continue. `verify` is [`verify`] for this install.
pub(crate) fn with_dialogs(
    available: Sources,
    prompter: &dyn Prompter,
    title_text: &str,
    verify: &mut dyn FnMut(bool, &mut dyn FnMut(&str, &str)) -> Answer,
) -> bool {
    let mut sign_in = false;
    loop {
        let refusal = match verify(sign_in, &mut |uri, code| {
            prompter.info(title_text, &code_prompt(uri, code));
        }) {
            Answer::Allowed => return true,
            Answer::Denied(refusal) => refusal,
        };
        let offers_sign_in =
            available.account && matches!(refusal, Refusal::NotOwned | Refusal::SignInRequired);
        // A stored account the service keeps refusing must still be replaceable.
        let switch = available.account && refusal == Refusal::Unverifiable;
        let question = if offers_sign_in {
            "Sign in with a Microsoft account now?"
        } else {
            "Try again?"
        };
        let mut text = format!("{}\n\n{}", title(refusal), body(refusal, available));
        if matches!(refusal, Refusal::NotInstalled | Refusal::NotOwned) {
            text.push_str(&format!("\n\nGet Minecraft: {}", policy::GET_MINECRAFT_URL));
        }
        text.push_str(&format!("\n\n{question}"));
        match prompter.confirm(title_text, &text) {
            Consent::Accepted => sign_in = offers_sign_in,
            Consent::Declined if switch => {
                match prompter.confirm(title_text, "Sign in with a different Microsoft account?") {
                    Consent::Accepted => sign_in = true,
                    Consent::Declined | Consent::Unavailable => return false,
                }
            }
            Consent::Declined => return false,
            Consent::Unavailable => {
                prompter.alert(title_text, &text);
                return false;
            }
        }
    }
}

#[cfg(test)]
mod tests;
