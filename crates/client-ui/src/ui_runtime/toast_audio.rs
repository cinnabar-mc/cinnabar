//! Notification sound follows the toast reaching the front of the presentation queue.

use super::UiRuntime;

impl UiRuntime {
    /// Expires retained HUD output and emits sounds when queued notifications start.
    pub fn expire_hud(&mut self, now: u64) {
        self.advance_toasts(now, |name| crate::sound_requests::ui_sound(name, 1.0, 1.0));
    }

    /// Advances notification presentation through the caller's sound sink.
    fn advance_toasts(&mut self, now: u64, mut sound: impl FnMut(&'static str)) {
        self.session.hud.expire(now);
        let Some(toast) = self
            .session
            .hud
            .toasts()
            .front()
            .filter(|toast| toast.visible_at(now))
        else {
            return;
        };
        let identity = (
            toast.fifo_sequence,
            toast.received_millis,
            toast.started_millis,
        );
        if self.session.last_sounded_toast != Some(identity) {
            self.session.last_sounded_toast = Some(identity);
            sound(crate::sound_requests::TOAST);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{HudEvent, UiEvent};
    use std::sync::Arc;

    fn receive(runtime: &mut UiRuntime, player: &mut player_state::PlayerState, sequence: u64) {
        runtime
            .apply(
                player,
                super::super::SequencedUiEvent {
                    session_id: 1,
                    fifo_sequence: sequence,
                    local_millis: 100,
                    server_tick: None,
                    event: UiEvent::Hud(HudEvent::Toast {
                        title: Arc::from("Notice"),
                        message: Arc::from("Message"),
                    }),
                },
            )
            .unwrap();
    }

    #[test]
    fn received_toasts_sound_once_when_their_queued_presentation_starts() {
        let mut runtime = UiRuntime::new(1);
        let mut player = player_state::PlayerState::new(1);
        receive(&mut runtime, &mut player, 1);
        receive(&mut runtime, &mut player, 2);
        let second = runtime.hud().toasts()[1].started_millis;
        let mut sounds = Vec::new();
        runtime.advance_toasts(99, |name| sounds.push(name));
        assert!(sounds.is_empty());
        runtime.advance_toasts(100, |name| sounds.push(name));
        assert_eq!(sounds, ["random.toast"]);
        runtime.advance_toasts(second - 1, |name| sounds.push(name));
        assert_eq!(sounds.len(), 1);
        runtime.advance_toasts(second, |name| sounds.push(name));
        assert_eq!(sounds, ["random.toast", "random.toast"]);
        runtime.advance_toasts(second + 1, |name| sounds.push(name));
        assert_eq!(sounds.len(), 2);
    }

    #[test]
    fn expired_and_retired_session_toasts_do_not_sound_later() {
        let mut runtime = UiRuntime::new(1);
        let mut player = player_state::PlayerState::new(1);
        receive(&mut runtime, &mut player, 1);
        let expires = runtime.hud().toasts()[0].expires_millis;
        let mut sounds = Vec::new();
        runtime.advance_toasts(expires, |name| sounds.push(name));
        assert!(sounds.is_empty());
        receive(&mut runtime, &mut player, 2);
        runtime.begin_session(2);
        runtime.advance_toasts(100, |name| sounds.push(name));
        assert!(sounds.is_empty());
        runtime.begin_session(1);
        receive(&mut runtime, &mut player, 1);
        runtime.advance_toasts(100, |name| sounds.push(name));
        assert_eq!(sounds, ["random.toast"]);
    }

    #[test]
    fn repeated_client_toasts_have_independent_queued_sounds() {
        let mut runtime = UiRuntime::new(1);
        for _ in 0..2 {
            runtime.push_client_toast(Arc::from("Client"), Arc::from("Message"), 100);
        }
        let second = runtime.hud().toasts()[1].started_millis;
        let mut sounds = Vec::new();
        runtime.advance_toasts(100, |name| sounds.push(name));
        runtime.advance_toasts(second, |name| sounds.push(name));
        runtime.advance_toasts(second, |name| sounds.push(name));
        assert_eq!(sounds, ["random.toast", "random.toast"]);
    }
}
