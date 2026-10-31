//! Native input imports and admission for text edits owned by one component.

use super::{Instance, State, cinnabar, controls};
use anyhow::Result;

impl cinnabar::extension::input::Host for State {
    /// Exposes only the current container identity under controls authority.
    fn read_container_screen(&mut self) -> Result<Result<Option<String>, String>> {
        self.charge()?;
        Ok(if self.grants.controls {
            Ok(self.controls.pointer.screen.clone())
        } else {
            Err("controls permission denied".into())
        })
    }
    /// Stages a bounded local pointer request for the observed container.
    fn position_pointer(&mut self, x: f32, y: f32) -> Result<Result<(), String>> {
        self.charge()?;
        Ok(if self.grants.controls {
            self.controls.pointer.stage(x, y)
        } else {
            Err("controls permission denied".into())
        })
    }

    /// Keeps native text payloads separate from the existing numeric event ABI.
    fn read_text_events(
        &mut self,
    ) -> Result<Result<Vec<cinnabar::extension::input::TextEvent>, String>> {
        controls::read_text_events(self)
    }
    /// Reads the same stable additive field batch throughout a frame callback.
    fn read_field_events(
        &mut self,
    ) -> Result<Result<Vec<cinnabar::extension::input::FieldEvent>, String>> {
        controls::read_field_events(self)
    }
    /// Reads only the host's one-frame action edge, never raw keyboard state.
    fn demo_pressed(&mut self) -> Result<bool> {
        Ok(self.pressed)
    }

    fn read_bindings(
        &mut self,
        after: Option<u64>,
        offset: u32,
        limit: u32,
    ) -> Result<Result<Option<crate::InputBindings>, String>> {
        super::input_bindings::read(self, after, offset, limit)
    }

    fn read_controls(&mut self) -> Result<Result<crate::ControlFrame, String>> {
        controls::read(self)
    }

    fn read_selected_controls(
        &mut self,
        selection: cinnabar::extension::input::Selection,
    ) -> Result<Result<crate::ControlFrame, String>> {
        controls::read_selected(self, selection)
    }

    fn reserve_keys(&mut self, keys: Vec<String>) -> Result<Result<(), String>> {
        controls::reserve(self, keys)
    }
}

impl Instance {
    /// Retains native committed text while a callback is deferred, without granting guest writes.
    pub(crate) fn queue_text_events(
        &mut self,
        events: Vec<ui::mod_panel::TextEvent>,
    ) -> Result<()> {
        anyhow::ensure!(
            self.is_active(),
            "native text events require an active component"
        );
        controls::queue_text_events(self.store.data_mut(), events)
    }
}

impl Instance {
    /// Admits bounded live field input only for an active component.
    pub(crate) fn queue_field_events(
        &mut self,
        events: Vec<ui::mod_panel::FieldEvent>,
    ) -> Result<()> {
        anyhow::ensure!(
            self.is_active(),
            "native field events require an active component"
        );
        controls::queue_field_events(self.store.data_mut(), events)
    }
}

/// Transactional one-shot output scoped to the currently observed container.
#[derive(Default)]
pub(crate) struct PointerState {
    screen: Option<String>,
    available: bool,
    pending: Option<[f32; 2]>,
    committed: Option<[f32; 2]>,
}
impl PointerState {
    /// Reconciles input ownership before each callback and invalidates departed requests.
    fn observe(&mut self, screen: Option<&str>, available: bool) {
        if self.screen.as_deref() != screen || !available {
            self.screen = screen.map(str::to_owned);
            self.pending = None;
            self.committed = None;
        }
        self.available = available;
    }
    /// Accepts finite normalized positions only while a container owns focused input.
    fn stage(&mut self, x: f32, y: f32) -> Result<(), String> {
        if self.screen.is_none() || !self.available {
            return Err("no focused container".into());
        }
        if ![x, y]
            .into_iter()
            .all(|v| v.is_finite() && (0.0..=1.0).contains(&v))
        {
            return Err("pointer position must be normalized".into());
        }
        self.pending = Some([x, y]);
        Ok(())
    }
    /// Publishes only successful callback output, retaining unconsumed requests.
    pub(crate) fn commit(&mut self) {
        if let Some(point) = self.pending.take() {
            self.committed = Some(point);
        }
    }
    /// Drops output staged by a failed callback.
    pub(crate) fn rollback(&mut self) {
        self.pending = None;
    }
}
impl crate::ModHost {
    /// Supplies the open container identity and native pointer eligibility independently of packages.
    pub fn observe_container_screen(&mut self, screen: Option<&str>, pointer_available: bool) {
        self.instance
            .store
            .data_mut()
            .controls
            .pointer
            .observe(screen, pointer_available);
    }
    /// Consumes one successful pointer request; inactive or revoked mods cannot move it.
    pub fn take_pointer_position(&mut self) -> Option<[f32; 2]> {
        let allowed = self.is_active() && self.grants().controls;
        self.instance
            .store
            .data_mut()
            .controls
            .pointer
            .committed
            .take()
            .filter(|_| allowed)
    }
}

#[cfg(test)]
mod pointer_tests {
    use super::{PointerState, State, cinnabar};
    use cinnabar::extension::input::Host;
    /// Both observation and writes require controls authority, even with an observed screen.
    #[test]
    fn pointer_imports_require_controls() {
        let mut state = State::new(
            crate::ModGrants::default(),
            String::new(),
            Default::default(),
        );
        state
            .controls
            .pointer
            .observe(Some("container.example"), true);
        assert!(state.read_container_screen().unwrap().is_err());
        assert!(state.position_pointer(0.25, 0.75).unwrap().is_err());
        state.grants.controls = true;
        assert_eq!(
            state.read_container_screen().unwrap().unwrap().as_deref(),
            Some("container.example")
        );
        state.position_pointer(0.25, 0.75).unwrap().unwrap();
        state.controls.commit();
        assert_eq!(state.controls.pointer.committed, Some([0.25, 0.75]));
        state.controls.revoke();
        assert!(state.controls.pointer.committed.is_none());
    }

    /// Pointer requests are bounded, transactional, consumed once and scoped to a screen.
    #[test]
    fn pointer_request_lifecycle() {
        let mut state = PointerState::default();
        assert!(state.stage(0.5, 0.5).is_err());
        state.observe(Some("container.example"), true);
        for point in [
            [f32::NAN, 0.5],
            [0.5, f32::INFINITY],
            [-0.1, 0.5],
            [0.5, 1.1],
        ] {
            assert!(state.stage(point[0], point[1]).is_err());
        }
        state.stage(0.25, 0.75).unwrap();
        assert!(state.committed.is_none());
        state.rollback();
        state.commit();
        assert!(state.committed.is_none());
        state.stage(0.25, 0.75).unwrap();
        state.commit();
        assert_eq!(state.committed.take(), Some([0.25, 0.75]));
        assert!(state.committed.take().is_none());
        state.stage(0.5, 0.5).unwrap();
        state.commit();
        state.observe(Some("container.example"), false);
        assert!(state.committed.is_none());
        assert!(state.stage(0.5, 0.5).is_err());
        assert_eq!(state.screen.as_deref(), Some("container.example"));
        state.observe(None, false);
        assert!(state.committed.is_none());
    }
}
