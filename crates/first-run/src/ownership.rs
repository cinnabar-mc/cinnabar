//! The ownership gate at the start of every packaged launch: play continues only when a proof
//! source this platform offers finds that the player owns Minecraft.

use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

use launcher::{
    install_layout::InstallLayout,
    ownership::{self as policy, Check, Sources, Store, Verdict},
};
use launcher_host::{
    native_dialog::{Consent, Prompter},
    ownership as sources,
};
use serde::Serialize;

use crate::{
    Progress,
    screen::{Action, Effect, Meter, Screen, refused_actions},
    status::Status,
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
    let available = sources::available();
    if !available.gated() {
        tracing::info!("ownership gate off: no proof source on this device in this build");
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
    let found = available.local_install.is_some() && sources::local_install_found();
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
    let available = sources::available();
    // A retry decides as the launch would: with no source left, the gate is off.
    if !available.gated() || (!sign_in && found_locally(available)) {
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

/// Where `store`'s install comes from, for refusals.
const fn install_text(store: Store) -> &'static str {
    match store {
        Store::Microsoft => {
            "On Windows, Cinnabar can use Minecraft installed from the Microsoft Store or the Xbox app. If you bought Minecraft for Windows or Minecraft: Java & Bedrock Edition for PC, it's already in your library there."
        }
        Store::Android => {
            "On Android, Cinnabar can use Minecraft installed from Google Play. A copy bought on another platform or store, such as the Amazon Appstore, doesn't count here. If Google Play already shows it as installed, update it there; if no update is available, back up your worlds, then reinstall it from Google Play."
        }
    }
}

/// The refusal's explanation for `available` sources, ending with the notice.
pub(crate) fn body(refusal: Refusal, available: Sources) -> String {
    let install = available.local_install.map(install_text);
    let mut text = match refusal {
        Refusal::NotInstalled => format!(
            "Cinnabar is for people who own Minecraft. {} Install it, then select Try again.",
            install.unwrap_or_default()
        ),
        Refusal::NotOwned => "The Microsoft account you signed in with doesn't own Minecraft for PC. A copy bought on a phone or console doesn't include PC. Sign in with the account that bought it, or get Minecraft.".to_owned(),
        Refusal::SignInRequired => "Cinnabar is for people who own Minecraft. Sign in once with the Microsoft account that owns Minecraft for PC so Cinnabar can confirm it. Afterwards you can play with any of your accounts.".to_owned(),
        Refusal::Unverifiable => "Cinnabar couldn't reach Microsoft to confirm that you own Minecraft. Check your connection, then try again.".to_owned(),
    };
    if let Some(install) = install.filter(|_| refusal != Refusal::NotInstalled) {
        text.push('\n');
        text.push_str(install);
    }
    text.push('\n');
    text.push_str(NOTICE);
    text
}

/// What a game started without its setup screen shows when the local install fails, as when
/// Android restores it after the process ended; its only way back is reopening the app.
pub(crate) fn restored_refusal() -> String {
    let install = sources::available()
        .local_install
        .map(install_text)
        .unwrap_or_default();
    format!(
        "{}\nCinnabar is for people who own Minecraft. {install} Install it, then close and reopen the app.\n{NOTICE}",
        title(Refusal::NotInstalled)
    )
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
            text.push_str(&format!(
                "\n\nGet Minecraft: {}",
                available.get_minecraft_url()
            ));
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

/// One gate screen for a host that draws its own setup interface.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Prompt {
    pub title: String,
    /// Paragraphs split on `\n`.
    pub body: String,
    /// In display order; the first is the default.
    pub buttons: Vec<Button>,
}

/// One button; one that neither opens a link nor quits answers the host's wait.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Button {
    pub label: String,
    /// A link to open, staying on the screen.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open: Option<String>,
    /// The app to try first for `open`, before any app that handles it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    /// Closes setup without playing.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub quit: bool,
}

impl Prompt {
    /// `store_app` opens "Get Minecraft" when it is installed.
    fn new(screen: &Screen, available: Sources, store_app: Option<&str>) -> Self {
        let button = |action: Action| Button {
            label: screen.button_label(action).to_owned(),
            open: match (action, screen) {
                (Action::GetMinecraft, _) => Some(available.get_minecraft_url().to_owned()),
                (Action::OpenLink, Screen::SignIn { uri, .. }) => Some(uri.clone()),
                _ => None,
            },
            app: store_app
                .filter(|_| action == Action::GetMinecraft)
                .map(str::to_owned),
            quit: action == Action::Quit,
        };
        Self {
            title: screen.title(false).to_owned(),
            body: screen.body(),
            buttons: screen.actions().iter().copied().map(button).collect(),
        }
    }
}

/// A setup interface that draws the gate's screens itself, as Android's setup activity does.
pub trait GateHost {
    /// Shows a report while a check runs.
    fn progress(&mut self, update: &Progress);
    /// Shows `prompt` while a check runs; its buttons only open links or quit.
    fn show(&mut self, prompt: &Prompt);
    /// Shows `prompt` and returns the index of the answering button pressed, or `None` once the
    /// host has closed.
    fn choose(&mut self, prompt: &Prompt) -> Option<usize>;
}

/// Runs the gate through `host` for a packaged install; returns whether play may continue.
/// Setting `cancel` ends it unanswered.
pub fn verify_with_host(
    layout: &InstallLayout,
    cancel: &AtomicBool,
    host: &mut dyn GateHost,
) -> bool {
    if !layout.is_installed() || open_without_asking(layout) {
        return true;
    }
    let available = sources::available();
    let store_app = available.store_app();
    with_host(
        available,
        store_app,
        cancel,
        host,
        &mut |sign_in, on_code| verify(layout, sign_in, cancel, on_code),
    )
}

/// [`verify_with_host`] after the silent checks, with `verify` for this install.
fn with_host(
    available: Sources,
    store_app: Option<&str>,
    cancel: &AtomicBool,
    host: &mut dyn GateHost,
    verify: &mut Verify<'_>,
) -> bool {
    let mut sign_in = false;
    loop {
        let checking = Status::awaiting_ownership();
        if let Some(update) = Progress::new(&checking, &mut Meter::default(), Instant::now(), false)
        {
            host.progress(&update);
        }
        let answer = verify(sign_in, &mut |uri, code| {
            let screen = Screen::SignIn {
                uri: uri.to_owned(),
                code: code.to_owned(),
            };
            host.show(&Prompt::new(&screen, available, store_app));
        });
        if cancel.load(Ordering::Relaxed) {
            return false;
        }
        let screen = match answer {
            Answer::Allowed => return true,
            Answer::Denied(refusal) => Screen::Refused {
                refusal,
                sources: available,
            },
        };
        let prompt = Prompt::new(&screen, available, store_app);
        sign_in = loop {
            let Some(&action) = host
                .choose(&prompt)
                .and_then(|index| screen.actions().get(index))
            else {
                return false;
            };
            match screen.on_action(action).1 {
                Effect::Verify { sign_in } => break sign_in,
                Effect::Quit => return false,
                // The host opens links itself.
                _ => {}
            }
        };
    }
}

#[cfg(test)]
mod tests;
