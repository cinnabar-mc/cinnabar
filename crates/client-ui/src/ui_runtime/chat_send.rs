//! The outbound chat queue: enqueue, packet projection and send acknowledgement.

use std::{collections::VecDeque, sync::Arc};

use protocol::{ChatPacketError, Packet, chat_input_packet};
use ui::{ChatSendError, ChatSendRequest};

use super::UiRuntime;

impl UiRuntime {
    pub fn pending_chat_sends(&self) -> &VecDeque<ChatSendRequest> {
        self.session.chat_sends.pending()
    }

    pub const fn dropped_unsent_chat_messages(&self) -> u64 {
        self.dropped_unsent_chat_messages
    }

    /// Sets the connection's login identity and its initial presentation name.
    pub fn set_chat_identity(&mut self, source_name: Arc<str>, xuid: Arc<str>) {
        self.session.local_player_display_name = Arc::clone(&source_name);
        self.session.chat_source_name = source_name;
        self.session.chat_xuid = xuid;
    }

    /// Updates the local actor's presentation name without changing outbound chat identity.
    pub fn set_local_player_display_name(&mut self, name: Arc<str>) {
        self.session.local_player_display_name = name;
    }

    /// Queues an editor submission after removing leading ASCII whitespace.
    pub fn queue_chat_send(&mut self, now_millis: u64) -> Result<ChatSendRequest, ChatSendError> {
        let message = self
            .session
            .chat_editor
            .as_str()
            .trim_start_matches(|c| matches!(c, '\t'..='\r' | ' '));
        let request = self
            .session
            .chat_sends
            .push(self.session.session_id, message, now_millis)?;
        self.chat_history.push(Arc::clone(&request.message));
        self.session.chat_editor.clear();
        self.session.chat_autocomplete.clear();
        self.session.pending_chat_autocomplete_request = None;
        Ok(request)
    }

    /// Queues exactly `message` as a sent line, leaving the editor's draft untouched.
    pub fn queue_chat_message(
        &mut self,
        message: &str,
        now_millis: u64,
    ) -> Result<ChatSendRequest, ChatSendError> {
        let request = self
            .session
            .chat_sends
            .push(self.session.session_id, message, now_millis)?;
        self.chat_history.push(Arc::clone(&request.message));
        Ok(request)
    }

    /// The first queued message not yet handed to the transport.
    pub fn next_chat_packet(&self) -> Result<Option<(u64, Packet)>, ChatPacketError> {
        self.session
            .chat_sends
            .pending()
            .get(self.session.in_flight_chat_sends)
            .map(|request| {
                chat_input_packet(
                    &self.session.chat_source_name,
                    &self.session.chat_xuid,
                    &request.message,
                )
                .map(|packet| (request.sequence, packet))
            })
            .transpose()
    }

    pub fn confirm_chat_send(&mut self, sequence: u64) -> bool {
        self.session.chat_sends.confirm_front(sequence)
    }

    /// Queued messages handed to the transport and awaiting its acknowledgement.
    pub const fn in_flight_chat_sends(&self) -> usize {
        self.session.in_flight_chat_sends
    }

    pub fn mark_chat_send_enqueued(&mut self, session: u64, sequence: u64) -> bool {
        if session != self.session.session_id
            || self
                .session
                .chat_sends
                .pending()
                .get(self.session.in_flight_chat_sends)
                .is_none_or(|request| request.session != session || request.sequence != sequence)
        {
            return false;
        }
        self.session.in_flight_chat_sends += 1;
        true
    }

    /// Acknowledgements arrive in send order, so only the oldest in-flight message confirms.
    pub fn acknowledge_chat_send(&mut self, session: u64, sequence: u64) -> bool {
        if self.session.in_flight_chat_sends == 0
            || session != self.session.session_id
            || !self.confirm_chat_send(sequence)
        {
            return false;
        }
        self.session.in_flight_chat_sends -= 1;
        true
    }

    /// A failed write leaves every unacknowledged message queued for a later flush.
    pub fn fail_chat_send(&mut self, session: u64, sequence: u64) -> bool {
        if session != self.session.session_id
            || !self
                .session
                .chat_sends
                .pending()
                .iter()
                .take(self.session.in_flight_chat_sends)
                .any(|request| request.sequence == sequence)
        {
            return false;
        }
        self.session.in_flight_chat_sends = 0;
        true
    }
}
