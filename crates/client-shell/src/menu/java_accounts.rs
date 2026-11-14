//! Host adapter for the install's Java Edition accounts. Actions go to the account worker,
//! and the Accounts dialog keeps the list fresh while it is open.

use std::time::{Duration, Instant};

use launcher_host::account_control::AccountControl;
use {
    super::MenuRuntime,
    launcher::menu::{MenuAction, MenuDialog},
};

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
}

impl MenuRuntime {
    /// Asks the core for the Java accounts, as opening the Accounts dialog does.
    pub(super) fn refresh_java_accounts(&mut self) {
        self.java_accounts
            .queued
            .push(bridge::JavaAccountAction::List);
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
        match control.java_accounts() {
            Some(Ok(accounts)) => self.feeds.java_accounts = accounts,
            Some(Err(())) => self.feeds.java_accounts.error = Some(UNREACHABLE.into()),
            None => {}
        }
        let watching =
            self.dialog == Some(MenuDialog::Accounts) && self.feeds.java_accounts.add.in_progress();
        let ui = &mut self.java_accounts;
        if !watching {
            ui.next_poll = None;
        } else if ui.next_poll.is_none_or(|next| now >= next) {
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
        }
    }
}

#[cfg(test)]
mod tests;
