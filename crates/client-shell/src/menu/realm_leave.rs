//! Host adapter for leaving a joined Realm. Requests run on the account worker.

use launcher_host::account_control::AccountControl;
use {
    super::MenuRuntime,
    launcher::menu::{MenuAction, MenuDialog, auth::AuthState, joined_realm},
};

/// One leave at a time; a ticket keeps a reply for an earlier request or account from applying.
#[derive(Debug, Default)]
pub(super) struct RealmLeaveUi {
    ticket: u64,
    /// The confirmed Realm id waiting to be sent to the worker.
    request: Option<u32>,
    /// The ticket, Realm id and name of the leave awaiting its outcome.
    pending: Option<(u64, u32, String)>,
    /// A failure waiting for the active menu dialog to close.
    failure: Option<String>,
    account_generation: Option<u64>,
}

impl RealmLeaveUi {
    /// The Realm id being left, so its Leave control stays disabled until the outcome arrives.
    pub(super) fn leaving(&self) -> Option<u32> {
        self.pending.as_ref().map(|(_, id, _)| *id)
    }
}

impl MenuRuntime {
    /// Asks to confirm leaving a joined Realm, then sends the confirmed request once.
    pub(super) fn activate_realm_leave(&mut self, action: MenuAction) {
        if self.current_auth().as_ref() != &AuthState::Authenticated
            || self.over_world()
            || self.realm_leave.pending.is_some()
        {
            return;
        }
        match action {
            MenuAction::LeaveRealmDialog(id) if joined_realm(&self.realms, id).is_some() => {
                self.dialog = Some(MenuDialog::LeaveRealm(id));
                self.focused = 0;
            }
            MenuAction::ConfirmLeaveRealm(id)
                if self.dialog == Some(MenuDialog::LeaveRealm(id)) =>
            {
                self.dialog = None;
                self.focused = 0;
                let Some(name) = joined_realm(&self.realms, id).map(|realm| realm.name.clone())
                else {
                    return;
                };
                let leave = &mut self.realm_leave;
                leave.ticket = leave.ticket.wrapping_add(1);
                leave.request = Some(id);
                leave.pending = Some((leave.ticket, id, name));
            }
            _ => {}
        }
    }

    /// Sends a confirmed leave and installs only the current signed-in account's outcome.
    pub(super) fn sync_realm_leave(&mut self, control: &mut dyn AccountControl) {
        let generation = control.account_generation();
        let signed_in = self.current_auth().as_ref() == &AuthState::Authenticated;
        let leave = &mut self.realm_leave;
        if !signed_in
            || leave
                .account_generation
                .zip(generation)
                .is_some_and(|(old, current)| old != current)
        {
            leave.request = None;
            leave.pending = None;
            leave.failure = None;
        }
        leave.account_generation = generation;
        if let Some(id) = self.realm_leave.request.take() {
            let ticket = self.realm_leave.ticket;
            if !control.request_realm_leave(ticket, id) {
                self.receive_realm_leave(ticket, Err(()));
            }
        }
        if let Some((ticket, result)) = control.realm_leave() {
            self.receive_realm_leave(ticket, result);
        }
        if !self.navigation_popup_open()
            && self.realm_membership.state.is_none()
            && !self.is_connecting()
            && self.disconnect_message.is_none()
            && let Some(message) = self.realm_leave.failure.take()
        {
            self.show_notice(&message);
        }
    }

    /// Drops the Realm the account left, or explains a failure so the player can retry.
    fn receive_realm_leave(&mut self, ticket: u64, result: Result<(), ()>) {
        let Some((_, id, name)) = self
            .realm_leave
            .pending
            .take_if(|(pending, _, _)| *pending == ticket)
        else {
            return;
        };
        match result {
            Ok(()) => {
                self.realms.retain(|realm| realm.leave_id() != Some(id));
                self.feeds.selected_realm = None;
                self.message = Some(format!("You left {name}."));
            }
            Err(()) => {
                self.realm_leave.failure =
                    Some(format!("Unable to leave {name}. Please try again."));
            }
        }
    }
}

#[cfg(test)]
mod tests;
