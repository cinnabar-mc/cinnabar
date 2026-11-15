//! Host adapter for the install's Java Edition accounts. Actions go to the account worker,
//! and the Accounts dialog keeps the list fresh while it is open.

use std::time::{Duration, Instant};

use launcher::java_accounts::{JavaAccount, JavaAccountAdd, JavaAccounts};
use launcher_host::account_control::AccountControl;
use {
    super::MenuRuntime,
    launcher::menu::{MenuAction, MenuDialog},
};

/// Placeholder Java profiles shown while developer recordings present accounts.
const PRESENTATION_ACCOUNTS: [(&str, &str); 2] = [
    ("00000000000000000000000000000001", "BlockBuilder"),
    ("00000000000000000000000000000002", "SkylineSurfer"),
];

/// How often the dialog asks for a Java sign-in's progress while it waits for the player.
const ADD_POLL: Duration = Duration::from_secs(1);

/// Shown when the core cannot be asked about the Java accounts.
const UNREACHABLE: &str = "Could not reach your Java accounts. Try again.";

#[derive(Debug, Default)]
pub(super) struct JavaAccountsUi {
    /// Actions waiting for the account worker.
    queued: Vec<bridge::JavaAccountAction>,
    /// When the progress of an add is next asked for.
    next_poll: Option<Instant>,
    /// Whether an action was sent and its answer has not arrived, so no poll piles up behind a
    /// core that is slow to answer.
    awaiting: bool,
}

/// The placeholder Java accounts presentation mode shows instead of the install's.
pub(super) fn presentation_accounts() -> JavaAccounts {
    JavaAccounts {
        accounts: PRESENTATION_ACCOUNTS
            .iter()
            .map(|(id, name)| JavaAccount {
                id: (*id).into(),
                name: (*name).into(),
            })
            .collect(),
        selected: Some(PRESENTATION_ACCOUNTS[0].0.into()),
        ..Default::default()
    }
}

impl MenuRuntime {
    /// Asks the core for the Java accounts, as opening the Accounts dialog does. Presentation
    /// mode shows placeholders instead, so no real profile can reach a capture.
    pub(super) fn refresh_java_accounts(&mut self) {
        if self.presentation_accounts {
            self.feeds.java_accounts = presentation_accounts();
            return;
        }
        self.java_accounts
            .queued
            .push(bridge::JavaAccountAction::List);
    }

    /// Clears a failed add's reason once the dialog that showed it closes.
    pub(super) fn dismiss_java_accounts(&mut self) {
        if matches!(self.feeds.java_accounts.add, JavaAccountAdd::Failed(_)) {
            self.feeds.java_accounts.add = JavaAccountAdd::Idle;
            if !self.presentation_accounts {
                self.java_accounts
                    .queued
                    .push(bridge::JavaAccountAction::Cancel);
            }
        }
        self.feeds.java_accounts.error = None;
    }

    /// Queues a Java account action from the Accounts dialog.
    pub(super) fn activate_java_account(&mut self, action: MenuAction) {
        if self.over_world() || self.is_connecting() {
            return;
        }
        let accounts = &self.feeds.java_accounts;
        let request = match action {
            MenuAction::AddJavaAccount => bridge::JavaAccountAction::Add,
            MenuAction::CancelJavaAccount => bridge::JavaAccountAction::Cancel,
            MenuAction::SwitchJavaAccount(index) => match accounts.accounts.get(index) {
                Some(account) => bridge::JavaAccountAction::Select(account.id.clone()),
                None => return,
            },
            MenuAction::RemoveJavaAccount => match &accounts.selected {
                Some(id) => bridge::JavaAccountAction::Remove(id.clone()),
                None => return,
            },
            _ => return,
        };
        self.dialog = Some(MenuDialog::Accounts);
        self.focused = 0;
        self.java_accounts.queued.push(request);
    }

    /// Installs the worker's answers, sends queued actions, and asks for an add's progress
    /// while the dialog shows it.
    pub(super) fn sync_java_accounts(&mut self, control: &mut dyn AccountControl, now: Instant) {
        let answer = control.java_accounts();
        if answer.is_some() {
            self.java_accounts.awaiting = false;
        }
        if self.presentation_accounts {
            // Real accounts never replace the placeholders, and nothing is sent.
            self.java_accounts.queued.clear();
            return;
        }
        match answer {
            Some(Ok(accounts)) => self.feeds.java_accounts = accounts,
            Some(Err(())) => self.feeds.java_accounts.error = Some(UNREACHABLE.into()),
            None => {}
        }
        let watching =
            self.dialog == Some(MenuDialog::Accounts) && self.feeds.java_accounts.add.in_progress();
        let ui = &mut self.java_accounts;
        if !watching {
            ui.next_poll = None;
        } else if !ui.awaiting && ui.next_poll.is_none_or(|next| now >= next) {
            ui.next_poll = Some(now + ADD_POLL);
            if ui.queued.is_empty() {
                ui.queued.push(bridge::JavaAccountAction::List);
            }
        }
        for action in ui.queued.drain(..) {
            if !control.request_java_accounts(action) {
                self.feeds.java_accounts.error = Some(UNREACHABLE.into());
                break;
            }
            ui.awaiting = true;
        }
    }
}

#[cfg(test)]
mod tests;
