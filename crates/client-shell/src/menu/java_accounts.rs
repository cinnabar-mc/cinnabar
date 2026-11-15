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
    /// The id of the account the removal question asks about, which a changed list follows.
    removing: Option<String>,
}

/// The placeholder Java accounts presentation mode shows instead of the install's.
pub(super) fn presentation_accounts() -> JavaAccounts {
    JavaAccounts {
        accounts: PRESENTATION_ACCOUNTS
            .iter()
            .map(|(id, name)| JavaAccount {
                id: (*id).into(),
                name: (*name).into(),
                picture_path: None,
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
            MenuAction::RemoveJavaAccount => {
                let selected = accounts.selected.as_ref().and_then(|id| {
                    accounts
                        .accounts
                        .iter()
                        .position(|account| &account.id == id)
                });
                if let Some(index) = selected {
                    self.java_accounts.removing = accounts.selected.clone();
                    self.dialog = Some(MenuDialog::RemoveJavaAccount(index));
                    self.focused = 0;
                }
                return;
            }
            MenuAction::ConfirmRemoveJavaAccount(index)
                if self.dialog == Some(MenuDialog::RemoveJavaAccount(index)) =>
            {
                match (
                    accounts.accounts.get(index),
                    self.java_accounts.removing.take(),
                ) {
                    (Some(account), Some(id)) if account.id == id => {
                        bridge::JavaAccountAction::Remove(id)
                    }
                    _ => return,
                }
            }
            MenuAction::OpenJavaSignInLink => {
                if let JavaAccountAdd::AwaitingCode { uri, .. } = &accounts.add
                    && uri.starts_with("https://")
                {
                    #[cfg(not(test))]
                    launcher_host::desktop::open_url(uri);
                }
                return;
            }
            _ => return,
        };
        self.dialog = Some(MenuDialog::Accounts);
        self.focused = 0;
        self.java_accounts.queued.push(request);
    }

    /// Keeps the removal question on the account it asked about when a new list moves it, and
    /// returns to the accounts once that account is gone.
    fn follow_removed_account(&mut self) {
        let Some(MenuDialog::RemoveJavaAccount(_)) = self.dialog else {
            return;
        };
        let accounts = &self.feeds.java_accounts.accounts;
        let position = self
            .java_accounts
            .removing
            .as_ref()
            .and_then(|id| accounts.iter().position(|account| &account.id == id));
        self.dialog = Some(match position {
            Some(index) => MenuDialog::RemoveJavaAccount(index),
            None => {
                self.java_accounts.removing = None;
                self.focused = 0;
                MenuDialog::Accounts
            }
        });
    }

    /// Installs the worker's answers, sends queued actions, and asks for an add's progress
    /// or the pictures being read while the dialog shows them.
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
            Some(Ok(accounts)) => {
                self.feeds.java_accounts = accounts;
                self.follow_removed_account();
            }
            Some(Err(())) => self.feeds.java_accounts.error = Some(UNREACHABLE.into()),
            None => {}
        }
        let java = &self.feeds.java_accounts;
        let watching = self.dialog == Some(MenuDialog::Accounts)
            && (java.add.in_progress() || java.pictures_pending);
        let ui = &mut self.java_accounts;
        if !watching {
            // An answer lost with a replaced worker must not keep the next watch from polling.
            ui.next_poll = None;
            ui.awaiting = false;
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
