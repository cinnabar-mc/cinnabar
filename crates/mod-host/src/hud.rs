//! Transactional HUD output: the mod's JSON-UI surface, hidden vanilla elements and local
//! feedback, with no input or world authority.

use super::{MAX_IMPORT_WRITES, State};
use anyhow::{Result, bail};
use std::sync::Arc;
use ui::mod_hud::{ElementStates, HiddenElements, HudViewport};
#[path = "hud/surface.rs"]
pub(super) mod surface;
#[cfg(test)]
#[path = "hud/surface_tests.rs"]
mod surface_tests;

/// Measures one line of surface label text as the host lays it out.
pub trait HudTextMeasure: Send + Sync {
    /// Width in GUI units at text scale 1, or none when `font` is unavailable.
    fn width(&self, text: &str, font: ui::mod_panel::LabelFont) -> Option<f32>;
}

#[derive(Default)]
pub(super) struct HudState {
    /// The committed surface, with its current bindings.
    pub surface: Option<Arc<ui::mod_panel::Surface>>,
    /// Process-unique identity of the committed surface.
    pub surface_revision: u64,
    pending_surface: Option<Option<Arc<ui::mod_panel::Surface>>>,
    /// The index of the last admitted surface document, which lets a control patch to it
    /// revalidate only what the patch changes.
    index: ui::mod_panel::HudIndex,
    pub hidden: HiddenElements,
    pending_hidden: Option<HiddenElements>,
    pub local_messages: Vec<String>,
    pending_messages: Vec<String>,
    /// Text measurements the running callback made.
    measures: u32,
    /// The HUD root of the last laid-out frame.
    pub viewport: Option<HudViewport>,
    /// Vanilla element states of the last laid-out frame.
    pub elements: ElementStates,
    pub text: Option<Arc<dyn HudTextMeasure>>,
}

impl HudState {
    /// Invalidates world projections without rebuilding unaffected retained HUD surfaces.
    pub(super) fn clear_world_projections(&mut self) {
        let mut changed = false;
        for surface in self
            .surface
            .iter_mut()
            .chain(self.pending_surface.iter_mut().flatten())
        {
            if surface
                .bindings
                .values()
                .any(|value| value.active_projection().is_some())
            {
                super::world_lifecycle::clear_surface_world(Arc::make_mut(surface));
                changed = true;
            }
        }
        if changed {
            self.surface_revision = Self::next_revision();
        }
    }

    /// Discards failed surface writes while retaining published output and native observations.
    pub(super) fn rollback(&mut self) {
        let Self {
            surface,
            surface_revision: _,
            pending_surface,
            index,
            hidden: _,
            pending_hidden,
            local_messages: _,
            pending_messages,
            measures,
            viewport: _,
            elements: _,
            text: _,
        } = self;
        *pending_surface = None;
        *pending_hidden = None;
        pending_messages.clear();
        *measures = 0;
        *index = surface
            .as_deref()
            .map(ui::mod_panel::HudIndex::new)
            .transpose()
            .expect("committed HUD surface was validated")
            .unwrap_or_default();
    }

    /// A process-unique revision for committed output that changed.
    fn next_revision() -> u64 {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    /// Drops everything the guest presented or staged, keeping the host's observations.
    pub(super) fn revoke(&mut self) {
        *self = Self {
            viewport: self.viewport,
            elements: self.elements,
            text: self.text.take(),
            ..Self::default()
        };
    }

    /// Resets per-callback read budgets.
    pub(super) fn begin_callback(&mut self) {
        self.measures = 0;
    }

    /// Publishes staged output after a successful callback.
    pub fn commit(&mut self) {
        let room = 4usize.saturating_sub(self.local_messages.len());
        self.local_messages
            .extend(self.pending_messages.drain(..).take(room));
        if let Some(surface) = self.pending_surface.take() {
            self.surface = surface;
            self.surface_revision = Self::next_revision();
        }
        if let Some(hidden) = self.pending_hidden.take() {
            self.hidden = hidden;
        }
    }
}

/// Charges one HUD import and checks hud authority.
pub(super) fn admit(state: &mut State) -> Result<Result<(), String>> {
    state.writes += 1;
    if state.writes > MAX_IMPORT_WRITES {
        bail!("HUD import budget exhausted");
    }
    Ok(if state.grants.hud {
        Ok(())
    } else {
        Err("HUD capability denied".into())
    })
}

/// Stages local system feedback without creating an outbound chat or command packet.
pub(super) fn local_message(state: &mut State, text: String) -> Result<Result<(), String>> {
    if let Err(error) = admit(state)? {
        return Ok(Err(error));
    }
    if text.is_empty()
        || text.len() > super::MAX_LABEL_BYTES
        || text.chars().any(|c| c.is_control() || c == '§')
    {
        return Ok(Err("local message must be short plain text".into()));
    }
    if state.hud.pending_messages.len() + state.hud.local_messages.len() >= 4 {
        return Ok(Err("local message queue is full".into()));
    }
    state.hud.pending_messages.push(text);
    Ok(Ok(()))
}

#[cfg(test)]
mod local_message_tests {
    use super::*;
    /// Messages are unavailable until commit and remain bounded without consumer progress.
    #[test]
    fn feedback_is_transactional_bounded_and_requires_hud_authority() {
        let mut state = State::new(
            super::super::ModGrants::default(),
            String::new(),
            Default::default(),
        );
        assert!(local_message(&mut state, "Notice".into()).unwrap().is_err());
        state.grants.hud = true;
        assert!(
            local_message(&mut state, "bad\nline".into())
                .unwrap()
                .is_err()
        );
        for _ in 0..4 {
            assert!(local_message(&mut state, "Notice".into()).unwrap().is_ok());
        }
        assert!(state.hud.local_messages.is_empty());
        assert!(local_message(&mut state, "Excess".into()).unwrap().is_err());
        state.hud.commit();
        assert_eq!(state.hud.local_messages, vec!["Notice"; 4]);
        assert!(local_message(&mut state, "Excess".into()).unwrap().is_err());
    }
}
