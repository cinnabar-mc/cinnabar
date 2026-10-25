//! Committed visual outputs of a loaded component.

use super::{CameraDelta, ModHost};
use std::sync::Arc;

impl ModHost {
    /// Reads a committed pulse so filtering can run before the frame outputs are collected.
    pub fn attack_pulse_requested(&self) -> bool {
        self.instance.attack_pulse_requested()
    }

    /// Reads a committed use pulse so filtering can run before the frame outputs are collected.
    pub fn use_pulse_requested(&self) -> bool {
        self.instance.use_pulse_requested()
    }

    /// Successfully committed local weather target, in rain/thunder channel order.
    pub fn weather_override(&self) -> Option<[f32; 2]> {
        use crate::runtime::cinnabar::extension::environment::Weather;
        self.instance.weather_override().map(|value| match value {
            Weather::Clear => [0., 0.],
            Weather::Rain => [1., 0.],
            Weather::Thunder => [1., 1.],
        })
    }
    /// Committed touch buttons; empty for an inactive instance or without controls authority.
    pub fn touch_buttons(&self) -> &[ui::mod_touch::TouchButton] {
        self.instance.touch_buttons()
    }
    /// Identifies this component instance to presented buttons; a reload gets a new token.
    pub fn touch_owner(&self) -> u64 {
        self.instance.touch_owner()
    }
    /// Whether the last executed frame callback received taps, including deferred ones.
    pub fn touch_presses_delivered(&self) -> bool {
        self.instance.touch_presses_delivered()
    }
    /// Queues a completed tap on one of this mod's buttons for its next frame callback.
    /// Returns false when the button is gone or the bounded queue is full.
    pub fn queue_touch_press(&mut self, id: &str) -> bool {
        self.instance.queue_touch_press(id)
    }
    /// Sprint and sneak held as if bound, from the current successful frame only.
    pub fn movement_intents(&self) -> Option<crate::GameplayMovementIntents> {
        self.instance.movement_intents()
    }
    /// Successfully committed local lighting override.
    pub fn fullbright(&self) -> bool {
        self.instance.fullbright()
    }

    /// Consumes the last successful frame's rotation once, without entering the guest.
    pub fn take_camera_delta(&mut self) -> Option<CameraDelta> {
        self.instance.take_camera_delta()
    }

    /// Committed render output and a process-unique generation that changes with it.
    pub fn render(&self) -> (&mod_render::RenderOutput, u64) {
        self.instance.render()
    }

    /// Returns only the last successfully committed plain-text label.
    pub fn label(&self) -> Option<&str> {
        self.instance.label()
    }

    /// The committed HUD surface with its current, patched bindings, and a process-unique
    /// revision that changes with every committed `set-surface` or `update-surface`. Item and
    /// projected values are resolved by the presenter, not here. Hosts and mod probes read it.
    pub fn hud_surface(&self) -> Option<(&Arc<ui::mod_panel::Surface>, u64)> {
        self.instance.hud_surface()
    }

    /// Vanilla HUD elements the committed output hides.
    pub fn hidden_hud_elements(&self) -> ui::mod_hud::HiddenElements {
        if self.is_active() {
            self.instance.hidden_hud_elements()
        } else {
            ui::mod_hud::HiddenElements::default()
        }
    }

    /// Records the last laid-out HUD root and vanilla element states that `read-viewport` and
    /// `read-elements` report to later callbacks.
    pub fn observe_hud(
        &mut self,
        viewport: Option<ui::mod_hud::HudViewport>,
        elements: ui::mod_hud::ElementStates,
    ) {
        self.instance.observe_hud(viewport, elements);
    }

    /// Records where the host last laid out this mod's panel, which `panel.read-layout`
    /// reports to later callbacks; none while the panel is not up.
    pub fn observe_panel_layout(&mut self, layout: Option<ui::mod_hud::PanelLayout>) {
        self.instance.observe_panel_layout(layout);
    }

    /// Installs the host label metrics `measure-text` uses; none reports them unavailable.
    pub fn set_text_measure(&mut self, text: Option<Arc<dyn crate::HudTextMeasure>>) {
        self.instance.set_text_measure(text);
    }

    /// Native presentation parameters from the last successful callbacks.
    pub fn presentation(&self) -> &ui::mod_presentation::ModPresentation {
        self.instance.presentation().0
    }

    /// Process-unique identity of the committed presentation, changing with it.
    pub fn presentation_revision(&self) -> u64 {
        self.instance.presentation().1
    }

    /// Supplies the installation's food table to `facts.read-item` and `facts.read-foods`;
    /// reloads keep it.
    pub fn set_food_catalog(
        &mut self,
        catalog: Option<&std::sync::Arc<assets::RuntimeEquipmentCatalog>>,
    ) {
        let current = self.instance.food_catalog();
        let unchanged = match (current.as_ref(), catalog) {
            (Some(current), Some(catalog)) => std::sync::Arc::ptr_eq(current, catalog),
            (current, catalog) => current.is_none() && catalog.is_none(),
        };
        if !unchanged {
            self.instance.set_food_catalog(catalog.cloned());
        }
    }

    /// Returns the committed visual override without entering the guest.
    pub fn time_override(&self) -> Option<u32> {
        self.instance.time_override()
    }
}
