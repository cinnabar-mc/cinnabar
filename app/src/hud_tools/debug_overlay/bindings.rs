//! Retained desktop controls and help text for the debug overlay.

use launcher::menu::settings_options::BindingId;
use std::sync::Arc;

use bevy::prelude::{ButtonInput, KeyCode, MouseButton};
use client_shell::physical_input::{keyboard_usage, mouse_button_code};
use launcher::menu::settings_options::{SettingsOptions, key_name};
use semantic_input::PhysicalControl;

/// An action run by pressing its suffix key while the debug key is held.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DebugChord {
    ReloadChunks,
    ClearChat,
    ChunkBorders,
    Help,
}

/// Each chord's suffix key and help text, in help order; detection and help both read it.
const CHORDS: [(DebugChord, KeyCode, &str); 4] = [
    (DebugChord::ReloadChunks, KeyCode::KeyA, "Reload chunks"),
    (DebugChord::ClearChat, KeyCode::KeyD, "Clear chat"),
    (
        DebugChord::ChunkBorders,
        KeyCode::KeyG,
        "Show chunk borders",
    ),
    (DebugChord::Help, KeyCode::KeyQ, "Show this help list"),
];

/// The chords pressed this frame, one bit per `CHORDS` row.
#[derive(Clone, Copy, Default)]
pub(super) struct DebugChords(u8);

impl DebugChords {
    pub(super) const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Yields each pressed chord with its suffix key, in help order.
    pub(super) fn iter(self) -> impl Iterator<Item = (DebugChord, KeyCode)> {
        CHORDS
            .iter()
            .enumerate()
            .filter(move |(row, _)| self.0 & (1 << row) != 0)
            .map(|(_, (chord, key, _))| (*chord, *key))
    }
}

pub(super) struct DebugBindings {
    settings: Option<Arc<SettingsOptions>>,
    control: Option<PhysicalControl>,
    hint: String,
}

impl Default for DebugBindings {
    fn default() -> Self {
        let settings = SettingsOptions::default();
        Self {
            settings: None,
            control: settings.named_key_control(BindingId::DebugOverlay),
            hint: binding_hint(&settings),
        }
    }
}

impl DebugBindings {
    /// Resolves changed settings once; retaining the Arc makes in-place edits copy on write.
    pub(super) fn update(&mut self, settings: Option<&Arc<SettingsOptions>>) {
        if match (&self.settings, settings) {
            (Some(previous), Some(current)) => Arc::ptr_eq(previous, current),
            (None, None) => true,
            _ => false,
        } {
            return;
        }
        let defaults = SettingsOptions::default();
        let resolved = settings.map_or(&defaults, Arc::as_ref);
        self.control = resolved.named_key_control(BindingId::DebugOverlay);
        self.hint = binding_hint(resolved);
        self.settings = settings.cloned();
    }

    /// Toggles on a fresh configured key or mouse press, never on a held control.
    pub(super) fn pressed(
        &self,
        keys: &ButtonInput<KeyCode>,
        mouse: Option<&ButtonInput<MouseButton>>,
    ) -> bool {
        match self.control {
            Some(PhysicalControl::KeyboardUsage(usage)) => keys
                .get_just_pressed()
                .any(|key| keyboard_usage(*key) == Some(usage)),
            Some(PhysicalControl::MouseButton(button)) => mouse.is_some_and(|mouse| {
                mouse
                    .get_just_pressed()
                    .any(|pressed| mouse_button_code(*pressed) == Some(button))
            }),
            _ => false,
        }
    }

    /// Whether the debug overlay key/button is currently held down.
    pub(super) fn is_down(
        &self,
        keys: &ButtonInput<KeyCode>,
        mouse: Option<&ButtonInput<MouseButton>>,
    ) -> bool {
        match self.control {
            Some(PhysicalControl::KeyboardUsage(usage)) => keys
                .get_pressed()
                .any(|key| keyboard_usage(*key) == Some(usage)),
            Some(PhysicalControl::MouseButton(button)) => mouse.is_some_and(|mouse| {
                mouse
                    .get_pressed()
                    .any(|pressed| mouse_button_code(*pressed) == Some(button))
            }),
            _ => false,
        }
    }

    /// Takes fresh chord suffixes, leaving the configured modifier's own press intact.
    pub(super) fn take_chords(
        &self,
        keys: &mut ButtonInput<KeyCode>,
        mouse: Option<&ButtonInput<MouseButton>>,
    ) -> DebugChords {
        let mut chords = DebugChords::default();
        if !self.is_down(keys, mouse) {
            return chords;
        }
        for (row, (_, key, _)) in CHORDS.iter().enumerate() {
            if self.control != keyboard_usage(*key).map(PhysicalControl::KeyboardUsage)
                && keys.clear_just_pressed(*key)
            {
                chords.0 |= 1 << row;
            }
        }
        chords
    }

    /// Formats one help row per chord, naming the configured debug key.
    pub(super) fn chord_help(&self) -> impl Iterator<Item = String> {
        let modifier = self.control.map(key_name).unwrap_or_default();
        CHORDS.iter().map(move |(_, key, help)| {
            let suffix = keyboard_usage(*key)
                .map(|usage| key_name(PhysicalControl::KeyboardUsage(usage)))
                .unwrap_or_default();
            format!("[Debug] {modifier} + {suffix} = {help}")
        })
    }

    /// Returns the help row already formatted for the current bindings.
    pub(super) fn hint(&self) -> &str {
        &self.hint
    }
}

/// Formats both shortcuts from the same persisted layout used to activate them.
fn binding_hint(settings: &SettingsOptions) -> String {
    let name = |key| {
        settings
            .named_key_control(key)
            .map(key_name)
            .unwrap_or_default()
    };
    format!(
        "{}: hide debug | {}: screenshot",
        name(BindingId::DebugOverlay),
        name(BindingId::Screenshot),
    )
}
