//! Final desktop held state, shared by gameplay gates and raw admission observations.

use super::*;
use crate::ModifierChord;

impl SemanticInputRouter {
    /// Re-evaluates final holds after ordered releases can re-arm quarantined controls.
    /// Edge modifiers remain event-local; held modifiers come only from admitted held keys.
    pub(super) fn finalize_desktop_admission(
        &self,
        frame: &DeviceFrame,
        mode: InputMode,
        sample: &mut Sample,
    ) -> Vec<PhysicalControl> {
        if mode != InputMode::KeyboardMouse {
            return Vec::new();
        }
        if self.pending_release_barrier || frame.window_focus_lost {
            return Vec::new();
        }
        let filtered = self.admitted_desktop_frame(frame);
        if filtered.keyboard_mouse.is_none() {
            return Vec::new();
        }
        // The final holds must use the same keys and modifiers we report. In particular,
        // a released or consumed modifier cannot keep a chord held through its edge flags.
        let pressed = sample.pressed;
        let released = sample.released;
        *sample = self.sample(&filtered, mode);
        sample.pressed = pressed;
        sample.released = released;
        if self.context != InputContext::Gameplay {
            return Vec::new();
        }
        let keyboard = filtered.keyboard_mouse.as_ref().expect("desktop sample");
        keyboard
            .keys
            .iter()
            .copied()
            .map(PhysicalControl::KeyboardUsage)
            .chain(
                keyboard
                    .mouse_buttons
                    .iter()
                    .copied()
                    .map(PhysicalControl::MouseButton),
            )
            .collect()
    }
    /// Filters desktop quarantine and derives modifiers from its current physical held keys.
    pub(super) fn admitted_desktop_frame(&self, frame: &DeviceFrame) -> DeviceFrame {
        let mut filtered = without_quarantined_controls(
            frame,
            &self.quarantined_controls,
            &[],
            &[],
            &self.settings,
        );
        let Some(keyboard) = filtered.keyboard_mouse.as_mut() else {
            return filtered;
        };
        keyboard.modifiers = ModifierChord {
            control: keyboard.keys.contains(&0xe0) || keyboard.keys.contains(&0xe4),
            shift: keyboard.keys.contains(&0xe1) || keyboard.keys.contains(&0xe5),
            alt: keyboard.keys.contains(&0xe2) || keyboard.keys.contains(&0xe6),
            super_key: keyboard.keys.contains(&0xe3) || keyboard.keys.contains(&0xe7),
        };
        filtered
    }
}
