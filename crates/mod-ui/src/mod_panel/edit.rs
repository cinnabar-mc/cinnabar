use super::widgets::*;
use super::*;
use serde_json::Value;

const MAX_NUMERIC_CHARS: usize = 24;

pub(super) enum Editor {
    Field {
        control: usize,
        rect: [f64; 4],
        state: Box<super::field::Field>,
        invalid: bool,
    },
}

impl ModUi {
    pub fn mod_panel_editing(&self) -> bool {
        self.mod_panel
            .as_ref()
            .is_some_and(|panel| panel.open && panel.edit.is_some())
    }

    /// Returns selected field text, an empty slice for a caret, or None without native field focus.
    pub fn mod_panel_field_selection(&self) -> Option<&str> {
        let panel = self.mod_panel.as_ref().filter(|panel| panel.open)?;
        let Editor::Field { state, .. } = panel.edit.as_ref()?;
        Some(&state.text[state.selection()])
    }

    /// Routes keyboard input exclusively to an open personal-panel editor.
    pub fn mod_panel_key(&mut self, key: &str, text: Option<&str>) -> Vec<Event> {
        self.mod_panel.as_mut().map_or_else(Vec::new, |panel| {
            panel.edit_key(key, text, Default::default())
        })
    }

    /// Routes a key with its physical modifier snapshot to the native field.
    pub fn mod_panel_field_key(
        &mut self,
        key: &str,
        text: Option<&str>,
        modifiers: ui::mod_panel::FieldModifiers,
    ) -> Vec<Event> {
        self.mod_panel
            .as_mut()
            .map_or_else(Vec::new, |panel| panel.edit_key(key, text, modifiers))
    }

    /// Reports native field focus for platform IME activation.
    pub fn mod_panel_field_focused(&self) -> bool {
        self.mod_panel
            .as_ref()
            .is_some_and(|panel| panel.open && matches!(panel.edit, Some(Editor::Field { .. })))
    }

    /// Reports the focused field's one authoritative IME composition state.
    pub fn mod_panel_composing(&self) -> bool {
        self.mod_panel_field_focused()
            && matches!(self.mod_panel.as_ref().and_then(|panel| panel.edit.as_ref()), Some(Editor::Field { state, .. }) if state.composing)
    }

    /// Places the platform's candidate popup below the last painted caret in window pixels.
    pub fn mod_panel_ime_position(&self) -> Option<[f32; 2]> {
        if !self.mod_panel_field_focused() {
            return None;
        }
        let Editor::Field { state, .. } = self.mod_panel.as_ref()?.edit.as_ref()?;
        Some(state.ime_position)
    }

    /// Publishes bounded inline IME text without emitting a value change before commit.
    pub fn mod_panel_preedit(&mut self, text: &str, cursor: Option<usize>) {
        if !self.mod_panel_field_focused() {
            return;
        }
        let Some(panel) = self.mod_panel.as_mut() else {
            return;
        };
        if let Some(Editor::Field { state, .. }) = &mut panel.edit {
            state.preedit(text, cursor);
        }
    }

    /// Commits IME text through the same selection and length checks as typing and paste.
    pub fn mod_panel_ime_commit(&mut self, text: &str) {
        if !self.mod_panel_field_focused() {
            return;
        }
        if let Some(panel) = self.mod_panel.as_mut()
            && let Some(Editor::Field {
                control,
                state,
                invalid,
                ..
            }) = &mut panel.edit
            && let Some((maximum, numeric)) = field_bounds(&panel.panel.controls[*control])
        {
            let was_invalid = *invalid;
            state.clear_composition();
            if state.insert(text, maximum, numeric) {
                *invalid = false;
                panel.live_change();
                if was_invalid {
                    panel.refresh_editor();
                }
            }
        }
    }

    pub fn cancel_mod_panel_edit(&mut self) {
        if let Some(panel) = self.mod_panel.as_mut() {
            panel.cancel_edit();
        }
    }
}

impl ModPanel {
    /// Native text fields retain keyboard ownership while a guest's capture flag is unchanged.
    pub(super) fn editing_text(&self) -> bool {
        matches!(self.edit.as_ref(),Some(Editor::Field {control,..})
            if matches!(self.panel.controls.get(*control),Some(Control::Text {..})))
    }
    /// Reconciles explicit guest replacements for persistent fields without discarding focus.
    pub(super) fn reconcile_editor_value(&mut self, next: &Panel) {
        let Some(Editor::Field {
            control,
            state,
            invalid,
            ..
        }) = &mut self.edit
        else {
            return;
        };
        let (
            Some(Control::Text {
                value: previous,
                live,
                navigation_keys,
                ..
            }),
            Some(Control::Text { value, .. }),
        ) = (
            self.panel.controls.get(*control),
            next.controls.get(*control),
        )
        else {
            return;
        };
        if (*live || *navigation_keys) && previous != value && state.text != *value {
            state.replace_value(value);
            let was_invalid = std::mem::replace(invalid, false);
            if was_invalid {
                self.refresh_editor();
            }
        }
    }

    pub(super) fn cancel_edit(&mut self) {
        if self.edit.take().is_some() {
            self.refresh_editor();
        }
    }

    /// Publishes literal draft data without putting user text through template resolution.
    fn refresh_editor(&mut self) {
        self.catalog = None;
        self.frame = None;
        self.drag = None;
        self.pointer = None;
    }

    /// Starts the native editor appropriate to the selected admitted control kind.
    pub(super) fn open_editor(&mut self, index: usize, rect: [f64; 4]) {
        self.edit = match self.panel.controls.get(index) {
            Some(Control::Slider { value, .. }) => {
                let text = format_number(*value);
                Some(Editor::Field {
                    control: index,
                    rect,
                    state: Box::new(super::field::Field::new(text)),
                    invalid: false,
                })
            }
            Some(Control::Text { value, .. }) => Some(Editor::Field {
                control: index,
                rect,
                state: Box::new(super::field::Field::new(value.clone())),
                invalid: false,
            }),
            _ => None,
        };
        self.refresh_editor();
    }

    /// Commits valid numeric and text fields through their separate event channels.
    fn commit_field(&mut self) -> Vec<Event> {
        let Some(Editor::Field {
            control,
            state,
            invalid,
            ..
        }) = self.edit.as_mut()
        else {
            return Vec::new();
        };
        let text = &state.text;
        if let Some(Control::Text {
            id,
            value,
            min_chars,
            max_chars,
            live,
            navigation_keys,
            ..
        }) = self.panel.controls.get_mut(*control)
        {
            if value == text && !*live && !*navigation_keys {
                self.cancel_edit();
                return Vec::new();
            }
            if ui::mod_panel::validate_text_value(text, *min_chars, *max_chars).is_err()
                || self.text_events.len() >= ui::mod_panel::MAX_PANEL_EVENTS
                || self.field_events.len() >= ui::mod_panel::MAX_PANEL_EVENTS
            {
                *invalid = true;
                self.refresh_editor();
                return Vec::new();
            }
            let value_changed = value != text;
            if value_changed {
                value.clone_from(text);
            }
            let event = ui::mod_panel::TextEvent {
                id: id.clone(),
                value: text.clone(),
            };
            self.text_events.push(event.clone());
            self.field_events
                .push(ui::mod_panel::FieldEvent::Submit(event));
            let live = *live || *navigation_keys;
            if value_changed {
                self.refresh_data();
            }
            if live {
                return Vec::new();
            }
            self.cancel_edit();
            return Vec::new();
        }
        let Some(Control::Slider {
            id,
            value,
            min,
            max,
            step,
            ..
        }) = self.panel.controls.get_mut(*control)
        else {
            self.cancel_edit();
            return Vec::new();
        };
        let parsed = text
            .parse::<f32>()
            .ok()
            .filter(|n| n.is_finite() && *n >= *min && *n <= *max);
        let Some(parsed) = parsed else {
            *invalid = true;
            self.refresh_editor();
            return Vec::new();
        };
        let next = (*min + ((parsed - *min) / *step).round() * *step).clamp(*min, *max);
        let event = Event {
            id: id.clone(),
            value: next,
        };
        *value = next;
        self.refresh_data();
        self.cancel_edit();
        vec![event]
    }

    /// Routes editing keys and optional navigation through one focused editor.
    fn edit_key(
        &mut self,
        key: &str,
        input: Option<&str>,
        modifiers: ui::mod_panel::FieldModifiers,
    ) -> Vec<Event> {
        if !self.open || self.edit.is_none() {
            return Vec::new();
        }
        if matches!(key, "Escape" | "F10" | "ShiftRight") {
            self.cancel_edit();
            return Vec::new();
        }
        if matches!(&self.edit, Some(Editor::Field { state, .. }) if state.composing) {
            return Vec::new();
        }
        let passthrough = self.field_navigation(key);
        if matches!(key, "Enter" | "NumpadEnter") {
            let id = self
                .edit
                .as_ref()
                .map(|Editor::Field { control, .. }| self.panel.controls[*control].id().to_owned());
            let events = self.commit_field();
            if passthrough && let Some(id) = id {
                self.forward_field_key(id, key, modifiers);
            }
            return events;
        }
        if passthrough {
            if let Some(Editor::Field { control, .. }) = &self.edit {
                self.forward_field_key(
                    self.panel.controls[*control].id().to_owned(),
                    key,
                    modifiers,
                );
            }
            return Vec::new();
        }
        if let Some(Editor::Field {
            state,
            control,
            invalid,
            ..
        }) = self.edit.as_mut()
        {
            if state.composing {
                return Vec::new();
            }
            let Some((limit, numeric)) = field_bounds(&self.panel.controls[*control]) else {
                return Vec::new();
            };
            let was_invalid = *invalid;
            let changed = if matches!(
                key,
                "SelectAll" | "Home" | "End" | "ArrowLeft" | "ArrowRight" | "Backspace" | "Delete"
            ) {
                state.key(key, modifiers)
            } else {
                let fallback = key
                    .strip_prefix("Digit")
                    .or_else(|| key.strip_prefix("Numpad"))
                    .filter(|s| s.len() == 1 && s.as_bytes()[0].is_ascii_digit())
                    .or(match key {
                        "Period" | "NumpadDecimal" => Some("."),
                        "Minus" | "NumpadSubtract" => Some("-"),
                        _ => None,
                    });
                if key == "Unidentified" || modifiers.super_key || modifiers.control {
                    return Vec::new();
                }
                input
                    .or(fallback)
                    .is_some_and(|input| state.insert(input, limit, numeric))
            };
            if changed {
                *invalid = false;
                self.live_change();
                if was_invalid {
                    self.refresh_editor();
                }
            }
        }
        Vec::new()
    }

    /// Returns whether this key belongs to the guest instead of native caret movement.
    fn field_navigation(&self, key: &str) -> bool {
        let Some(Editor::Field { control, state, .. }) = &self.edit else {
            return false;
        };
        matches!(
            self.panel.controls.get(*control),
            Some(Control::Text {
                navigation_keys: true,
                ..
            })
        ) && !state.composing
            && ui::mod_panel::is_field_navigation_key(key)
            && match key {
                "ArrowLeft" => state.selection().is_empty() && state.cursor == 0,
                "ArrowRight" => state.selection().is_empty() && state.cursor == state.text.len(),
                _ => true,
            }
    }

    /// Retains a bounded key event with the modifier snapshot taken at its input edge.
    fn forward_field_key(
        &mut self,
        id: String,
        key: &str,
        modifiers: ui::mod_panel::FieldModifiers,
    ) {
        if self.field_events.len() < ui::mod_panel::MAX_PANEL_EVENTS {
            self.field_events
                .push(ui::mod_panel::FieldEvent::Key(ui::mod_panel::FieldKey {
                    id,
                    key: key.into(),
                    modifiers,
                }));
        }
    }

    /// Retains exactly the valid values in the emitted batch, matching host-side admission.
    pub(super) fn drain_field_events(&mut self) -> Vec<ui::mod_panel::FieldEvent> {
        use ui::mod_panel::FieldEvent;
        let events = std::mem::take(&mut self.field_events);
        for event in &events {
            let (FieldEvent::Change(event) | FieldEvent::Submit(event)) = event else {
                continue;
            };
            let Some((
                index,
                Control::Text {
                    value,
                    min_chars,
                    max_chars,
                    ..
                },
            )) = self
                .panel
                .controls
                .iter_mut()
                .enumerate()
                .find(|(_, control)| control.id() == event.id)
            else {
                continue;
            };
            if value.as_str() != event.value.as_str()
                && ui::mod_panel::validate_text_value(&event.value, *min_chars, *max_chars).is_ok()
            {
                value.clone_from(&event.value);
                Arc::make_mut(&mut self.data).set_global(
                    format!("#row_{index}_value"),
                    json_ui::Scalar::Text(event.value.clone()),
                );
            }
        }
        events
    }

    /// Coalesces live values by field until the app drains this frame's native batch.
    fn live_change(&mut self) {
        let Some(Editor::Field { control, state, .. }) = &self.edit else {
            return;
        };
        let Some(Control::Text { id, live: true, .. }) = self.panel.controls.get(*control) else {
            return;
        };
        let event = ui::mod_panel::TextEvent {
            id: id.clone(),
            value: state.text.clone(),
        };
        self.field_events.retain(|existing| !matches!(existing, ui::mod_panel::FieldEvent::Change(old) if old.id == event.id));
        if self.field_events.len() < ui::mod_panel::MAX_PANEL_EVENTS {
            self.field_events
                .push(ui::mod_panel::FieldEvent::Change(event));
        }
    }
}

/// Shares character capacity and numeric admission across typing, paste and explicit commits.
fn field_bounds(control: &Control) -> Option<(usize, bool)> {
    match control {
        Control::Text { max_chars, .. } => Some((*max_chars, false)),
        Control::Slider { .. } => Some((MAX_NUMERIC_CHARS, true)),
        _ => None,
    }
}

/// Uses the finite native slider value's shortest ordinary representation.
fn format_number(value: f32) -> String {
    value.to_string()
}

/// Text scrolls inside its authored field; numeric entry retains a usable minimum width.
fn field_width(spec: &Panel, control: usize, rect: &[f64; 4], viewport: [f64; 2]) -> f64 {
    let minimum = if matches!(spec.controls.get(control), Some(Control::Text { .. })) {
        8.
    } else {
        42.
    };
    rect[2].max(minimum).min(viewport[0] - 8.)
}
pub(super) fn append_overlay(
    document: &mut Value,
    spec: &Panel,
    viewport: [f64; 2],
    editor: Option<&Editor>,
) {
    let Some(editor) = editor else {
        return;
    };
    let palette = Palette::for_panel(spec);
    let mut controls = vec![named(
        "dismiss",
        button("mod.dismiss", viewport, [0.; 2], Vec::new()),
    )];
    let Editor::Field {
        control,
        rect,
        invalid,
        ..
    } = editor;
    let is_text = matches!(spec.controls.get(*control), Some(Control::Text { .. }));
    let width = field_width(spec, *control, rect, viewport);
    let x = if is_text {
        rect[0]
    } else {
        rect[0] + rect[2] - width
    }
    .clamp(4., viewport[0] - width - 4.);
    let height = 17.;
    let y = (rect[1] + (rect[3] - height) * 0.5).clamp(4., viewport[1] - height - 4.);
    let mut field = chrome([width, height], palette, 3.);
    field.push(named(
        "value",
        serde_json::json!({
            "type":"custom", "renderer":super::field_paint::RENDERER,
            "size":[width - 6., 13.], "offset":[3., 2.],
            "anchor_from":"top_left", "anchor_to":"top_left", "clip_children":true
        }),
    ));
    controls.push(named(
        "number",
        button("mod.editor", [width, height], [x, y], field),
    ));
    let message = if *invalid {
        match spec.controls.get(*control) {
            Some(Control::Slider { min, max, .. }) => Some(format!(
                "Use {} to {}",
                format_number(*min),
                format_number(*max)
            )),
            Some(Control::Text {
                min_chars,
                max_chars,
                ..
            }) => Some(format!("Use {min_chars} to {max_chars} characters")),
            _ => None,
        }
    } else {
        None
    };
    if let Some(message) = message {
        let w = 105_f64.min(viewport[0] - 8.);
        let x = x.min(viewport[0] - w - 4.);
        let y = (y + height + 2.).min(viewport[1] - 22.);
        let mut hint = chrome([w, 18.], palette, 3.);
        hint.push(named(
            "label",
            label(&message, [w - 6., 14.], [3., 4.], palette.accent, false),
        ));
        controls.push(named("hint", panel([w, 18.], [x, y], hint)));
    }
    document["panel"]["controls"]
        .as_array_mut()
        .expect("screen controls are an array")
        .extend(controls);
}
