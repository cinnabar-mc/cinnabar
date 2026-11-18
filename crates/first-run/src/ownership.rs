//! The ownership gate at the start of every packaged launch: play continues only when a proof
//! source this platform offers finds that the player owns Minecraft.

use std::sync::atomic::AtomicBool;

use launcher::{
    install_layout::InstallLayout,
    ownership::{self as policy, Check, Sources, Verdict},
};
use launcher_host::{
    native_dialog::{Consent, Prompter},
    ownership as sources,
};

use crate::screen::{Action, refused_actions};

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
    if found_locally(available) {
        return true;
    }
    let Some(client_id) = available.client_id() else {
        return false;
    };
    if !sources::pass_valid(layout) {
        return false;
    }
    tracing::info!(source = "account_pass", "ownership verified");
    let layout = layout.clone();
    let _ = std::thread::Builder::new()
        .name("ownership-recheck".into())
        .spawn(move || {
            sources::check_and_record(
                &layout,
                client_id,
                false,
                &AtomicBool::new(false),
                |_, _| {},
            );
        });
    true
}

/// Whether `available` includes a local install and it was found.
fn found_locally(available: Sources) -> bool {
    let found = available.local_install && sources::local_install_found();
    if found {
        tracing::info!(source = "local_install", "ownership verified");
    }
    found
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
    if !sign_in && found_locally(available) {
        return Answer::Allowed;
    }
    let Some(client_id) = available.client_id() else {
        tracing::info!(result = "not_installed", "ownership not verified");
        return Answer::Denied(Refusal::NotInstalled);
    };
    match sources::check_and_record(layout, client_id, sign_in, cancel, on_code) {
        Verdict::Allow => Answer::Allowed,
        Verdict::Deny(check) => Answer::Denied(match check {
            Check::NotOwned => Refusal::NotOwned,
            Check::SignInRequired => Refusal::SignInRequired,
            Check::Verified | Check::Unverifiable => Refusal::Unverifiable,
        }),
    }
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
        "Open {uri} and enter the code {code}, then return here. Sign in with any of your Microsoft accounts that owns Minecraft for PC; it doesn't have to be the one you play on. You only need to do this once."
    )
}

/// One gate check: whether to sign in, and where to show a sign-in code's URI and code.
pub(crate) type Verify<'a> = dyn FnMut(bool, &mut dyn FnMut(&str, &str)) -> Answer + 'a;

/// Runs the gate with native dialogs when no setup window can open; returns whether play may
/// continue. `verify` is [`verify`] for this install.
pub(crate) fn with_dialogs(
    available: Sources,
    prompter: &dyn Prompter,
    title_text: &str,
    verify: &mut Verify<'_>,
) -> bool {
    let mut sign_in = false;
    loop {
        // A blocking dialog, so the code stays readable; the check keeps polling meanwhile.
        let refusal = match verify(sign_in, &mut |uri, code| {
            prompter.alert(title_text, &code_prompt(uri, code));
        }) {
            Answer::Allowed => return true,
            Answer::Denied(refusal) => refusal,
        };
        // The setup window's buttons for this refusal, asked one question at a time.
        let actions = refused_actions(refusal, available);
        let offers_sign_in = actions.first() == Some(&Action::SignIn);
        // A stored account the service keeps refusing must still be replaceable.
        let switch = !offers_sign_in && actions.contains(&Action::SignIn);
        let question = if offers_sign_in {
            "Sign in with a Microsoft account now?"
        } else {
            "Try again?"
        };
        let mut text = format!("{}\n\n{}", title(refusal), body(refusal, available));
        if actions.contains(&Action::GetMinecraft) {
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
            // Declining sign-in still leaves the local install to check again.
            Consent::Declined if offers_sign_in && actions.contains(&Action::Retry) => {
                match prompter.confirm(title_text, "Try again?") {
                    Consent::Accepted => sign_in = false,
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
