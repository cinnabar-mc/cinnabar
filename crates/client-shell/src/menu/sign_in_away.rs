//! Carries a Microsoft sign-in across the player's trip to the browser. Phones end a large game
//! left for another app, so the client notes the code its player left with, resumes that
//! sign-in at the next launch, and asks the platform to keep it alive while they are away.

use std::{path::PathBuf, time::SystemTime};

use launcher::{
    menu::{
        MenuDialog,
        auth::{AuthState, select_auth},
    },
    pending_sign_in::{self, LeftWaiting},
};

use super::{AuthSupervisor, MenuRuntime};

/// One client's record of the sign-in its player is away for.
#[derive(Debug)]
pub(super) struct SignInAway {
    /// Whether the system ends this client while it is hidden. Only there does the client note
    /// the code and resume it at launch: elsewhere a second client may share the install.
    pub(super) resumes_at_launch: bool,
    /// The launch's one look for a sign-in left waiting has happened.
    checked: bool,
    /// The token cache and code this client noted; the code is empty once only an unfinished
    /// Add account's token remains.
    noted: Option<(PathBuf, String)>,
    /// The code whose helper already stopped once without settling it and was restarted.
    restarted: Option<String>,
    /// Whether the platform was last asked to keep the client alive.
    held: bool,
    /// Every request made to the platform, in order.
    #[cfg(test)]
    pub(super) hold_requests: Vec<bool>,
}

impl Default for SignInAway {
    fn default() -> Self {
        Self {
            resumes_at_launch: launcher::SYSTEM_ENDS_HIDDEN_CLIENT,
            checked: false,
            noted: None,
            restarted: None,
            held: false,
            #[cfg(test)]
            hold_requests: Vec::new(),
        }
    }
}

impl SignInAway {
    /// Asks the platform to keep the client alive while `waiting`, once per change. `renew`
    /// repeats the request for a sign-in already waiting, restarting the platform's time limit
    /// when the player leaves for the browser again.
    fn hold(&mut self, waiting: bool, renew: bool) {
        if waiting == self.held && !(waiting && renew) {
            return;
        }
        self.held = waiting;
        #[cfg(test)]
        self.hold_requests.push(waiting);
        request_hold(waiting);
    }

    /// Replaces the note with `code` for `cache`, when it differs.
    fn note(&mut self, cache: PathBuf, code: &str) {
        if self
            .noted
            .as_ref()
            .is_some_and(|(noted, saved)| *noted == cache && saved == code)
        {
            return;
        }
        if let Some((previous, _)) = self.noted.take()
            && previous != cache
        {
            let _ = pending_sign_in::forget_waiting(&previous);
        }
        // A failed write is still recorded, so it is tried and reported once per code.
        if let Err(error) = pending_sign_in::note_waiting(&cache, code) {
            bevy::log::warn!(%error, "sign-in will not resume if the system ends the client");
        }
        self.noted = Some((cache, code.to_owned()));
    }

    /// Removes this client's note, if it made or resumed one, ending its sign-in's restart
    /// allowance with it.
    pub(super) fn forget(&mut self) {
        self.restarted = None;
        if let Some((cache, _)) = self.noted.take() {
            let _ = pending_sign_in::forget_waiting(&cache);
        }
    }
}

/// Asks the Android activity to keep the process important while the player approves the code in
/// the browser; other platforms have no such request.
#[cfg(target_os = "android")]
fn request_hold(waiting: bool) {
    if let Err(error) = crate::mobile_input::android::with_activity(|env, activity| {
        env.call_method(activity, "setSignInWaiting", "(Z)V", &[waiting.into()])
            .map(drop)
    }) {
        static WARNING: std::sync::Once = std::sync::Once::new();
        WARNING.call_once(|| {
            bevy::log::warn!("Cannot keep the client alive during sign-in: {error}");
        });
    }
}

#[cfg(not(target_os = "android"))]
fn request_hold(_waiting: bool) {}

impl MenuRuntime {
    /// Once per launch, before any other sign-in, resumes the sign-in this client's player was
    /// waiting on when the system ended it. The helper polls the same code again, so approving
    /// it in the browser still signs them in, and the prompt says so. A cancelled sign-in left
    /// no note and stays closed; a token that arrived meanwhile is picked up by the saved
    /// account's check, or completes the unfinished Add account. When a note exists, a dialog
    /// the launch opened, or an account change under way, defers the look until it closes, so
    /// the prompt is visible; [`Self::sign_in_resume_deferred`] holds other sign-ins meanwhile.
    pub(super) fn resume_sign_in_left_waiting(&mut self, now: SystemTime) {
        if !self.sign_in_away.resumes_at_launch
            || self.sign_in_away.checked
            || self.external.is_some()
        {
            return;
        }
        #[cfg(feature = "developer-control")]
        if self.fixture_active() {
            self.sign_in_away.checked = true;
            return;
        }
        let primary = self.layout.auth_cache();
        let adding = self.account_store().pending_cache();
        if !pending_sign_in::waiting_path(&primary).exists()
            && !pending_sign_in::waiting_path(&adding).exists()
        {
            self.sign_in_away.checked = true;
            return;
        }
        if self
            .dialog
            .is_some_and(|dialog| dialog != MenuDialog::Accounts)
            || self.account_change_pending()
        {
            return;
        }
        self.sign_in_away.checked = true;
        if self.auth_attempted || self.auth_process.is_some() {
            return;
        }
        match pending_sign_in::left_waiting(&primary, now) {
            Some(LeftWaiting::Awaiting(code)) => {
                self.sign_in_away.noted = Some((primary, code));
                self.start_sign_in();
                return;
            }
            Some(LeftWaiting::Settled) => {
                let _ = pending_sign_in::forget_waiting(&primary);
            }
            None => {}
        }
        let code = match pending_sign_in::left_waiting(&adding, now) {
            Some(LeftWaiting::Awaiting(code)) => code,
            // The token arrived while the client was away; the account was not yet added.
            Some(LeftWaiting::Settled) if adding.is_file() => String::new(),
            Some(LeftWaiting::Settled) => {
                let _ = pending_sign_in::forget_waiting(&adding);
                return;
            }
            None => return,
        };
        self.sign_in_away.noted = Some((adding, code));
        self.resume_add_account();
    }

    /// Whether a launch's resume is waiting for a dialog or account change to close. The saved
    /// account's background check waits with it, so it cannot claim the launch's first sign-in.
    pub(super) fn sign_in_resume_deferred(&self) -> bool {
        self.sign_in_away.resumes_at_launch && !self.sign_in_away.checked
    }

    /// Keeps the client's note and the platform's hold in step with its sign-in at `now`. The
    /// note is written when the helper shows a code, before the browser opens, and removed once
    /// that sign-in finishes, fails or is cancelled; an Add account keeps it until the account is
    /// added. Nothing is read or written while the state is unchanged.
    pub(super) fn track_sign_in_away(&mut self, now: SystemTime) {
        if self.external.is_some() {
            return;
        }
        #[cfg(feature = "developer-control")]
        if self.fixture_active() {
            return;
        }
        let stopped = self.sign_in_cancelled || self.owned_sign_in_failure().is_some();
        let helper = self.auth_process.as_ref().map(AuthSupervisor::state);
        if self.sign_in_away.resumes_at_launch {
            match helper {
                _ if stopped => self.sign_in_away.forget(),
                _ if self.auth_restart_requested => {}
                Some(AuthState::AwaitingCode { code, .. }) => {
                    let (cache, code) = (self.sign_in_cache(), code.clone());
                    self.sign_in_away.note(cache, &code);
                }
                Some(AuthState::Checking) => {}
                Some(AuthState::Failed(_)) => {
                    if self.restart_stopped_helper(now) {
                        return;
                    }
                    self.sign_in_away.forget();
                }
                // An Add account keeps its note until the account is added or restored.
                _ if self.account_change_pending() => {}
                _ => self.sign_in_away.forget(),
            }
        }
        let helper = self.auth_process.as_ref().map(AuthSupervisor::state);
        // A helper restarting on the same code keeps a hold it already has, but never starts one.
        let restarting = self.auth_restart_requested || matches!(helper, Some(AuthState::Checking));
        let awaiting = !self.auth_restart_requested
            && select_auth(helper, self.control_auth.as_ref()).awaiting_browser();
        let waiting = !stopped && (awaiting || (restarting && self.sign_in_away.held));
        self.sign_in_away.hold(waiting, false);
    }

    /// Restarts a helper that stopped without settling the noted code, as when the system ends
    /// the helper process while the player is in the browser. A helper that settles its code
    /// removes the saved prompt, so a prompt still saved for the noted code means it was never
    /// answered. Each code gets one restart; a second stop shows its failure.
    fn restart_stopped_helper(&mut self, now: SystemTime) -> bool {
        let cache = self.sign_in_cache();
        let Some((noted, code)) = self.sign_in_away.noted.as_ref() else {
            return false;
        };
        if *noted != cache
            || code.is_empty()
            || self.sign_in_away.restarted.as_ref() == Some(code)
            || pending_sign_in::left_waiting(&cache, now)
                != Some(LeftWaiting::Awaiting(code.clone()))
        {
            return false;
        }
        bevy::log::warn!("sign-in helper stopped while waiting for approval; restarting it");
        self.sign_in_away.restarted = Some(code.clone());
        self.start_sign_in();
        true
    }

    /// Restarts the platform's hold when the player leaves for the browser again.
    pub(super) fn renew_sign_in_hold(&mut self) {
        if self.sign_in_away.held {
            self.sign_in_away.hold(true, true);
        }
    }
}
