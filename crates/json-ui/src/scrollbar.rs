//! Cinnabar scrollbar extensions for mod surfaces: hover and pressed state children on a
//! scroll view's box and track, a minimum box length, a wheel easing duration and an idle
//! auto-hide for mouse users. Like paint transforms, only catalogs opted in with
//! [`crate::Catalog::with_paint_transforms`] keep these properties, so resource packs and
//! server JSON-UI lay out and paint exactly as before.

use serde_json::Value;

use crate::tree::ResolvedControl;
use crate::widgets::{bound_bool, bound_number};

/// Scroll view property: the box's minimum length as a fraction of the track, `0..=1`.
pub const MIN_THUMB_PROPERTY: &str = "cinnabar_min_thumb_fraction";
/// The minimum box length when a view declares none, a tenth of the track.
pub const DEFAULT_MIN_THUMB: f64 = 0.1;
/// Scroll view property: seconds a wheel notch eases over under panel motion; zero is instant.
pub const WHEEL_SECONDS_PROPERTY: &str = "cinnabar_scroll_seconds";
/// Longest wheel easing a view may declare.
pub const MAX_WHEEL_SECONDS: f64 = 1.0;
/// Scroll view property: `true` hides the box until the view scrolls or the pointer is on
/// its box or track, then fades it out as a touch-mode box does.
pub const AUTO_HIDE_PROPERTY: &str = "cinnabar_scrollbar_auto_hide";

/// `scrollbar_box`/`scroll_track` property naming the child shown while it is pressed.
pub const PRESSED_STATE: &str = "cinnabar_pressed_control";
/// `scrollbar_box`/`scroll_track` property naming the child shown while it is hovered.
pub const HOVER_STATE: &str = "cinnabar_hover_control";
/// `scrollbar_box`/`scroll_track` property naming the child shown otherwise.
pub const DEFAULT_STATE: &str = "cinnabar_default_control";
/// The state properties, from the most specific state to the resting one.
pub const STATE_PROPERTIES: [&str; 3] = [PRESSED_STATE, HOVER_STATE, DEFAULT_STATE];
/// Control types that take [`STATE_PROPERTIES`].
pub const STATE_TYPES: [&str; 2] = ["scrollbar_box", "scroll_track"];

/// Every property this module adds, which catalogs without the opt-in drop.
pub(crate) const PROPERTIES: [&str; 6] = [
    MIN_THUMB_PROPERTY,
    WHEEL_SECONDS_PROPERTY,
    AUTO_HIDE_PROPERTY,
    PRESSED_STATE,
    HOVER_STATE,
    DEFAULT_STATE,
];

/// Whether `value` is an admissible [`MIN_THUMB_PROPERTY`].
pub fn accepts_min_thumb(value: f64) -> bool {
    (0.0..=1.0).contains(&value)
}

/// Whether `value` is an admissible [`WHEEL_SECONDS_PROPERTY`].
pub fn accepts_wheel_seconds(value: f64) -> bool {
    (0.0..=MAX_WHEEL_SECONDS).contains(&value)
}

/// The view's minimum box fraction, clamped into `0..=1`; the default when absent or not finite.
pub(crate) fn min_thumb(view: &ResolvedControl) -> f64 {
    bound_number(view, MIN_THUMB_PROPERTY).map_or(DEFAULT_MIN_THUMB, |value| value.clamp(0.0, 1.0))
}

/// Whether the view declares a minimum box length, literal or bound.
pub(crate) fn declares_min_thumb(view: &ResolvedControl) -> bool {
    bound_number(view, MIN_THUMB_PROPERTY).is_some()
}

/// The view's wheel easing in seconds, clamped into `0..=MAX_WHEEL_SECONDS`, when it declares one.
pub(crate) fn wheel_seconds(view: &ResolvedControl) -> Option<f64> {
    bound_number(view, WHEEL_SECONDS_PROPERTY).map(|value| value.clamp(0.0, MAX_WHEEL_SECONDS))
}

/// Whether the view hides its idle box for every input mode.
pub(crate) fn auto_hide(view: &ResolvedControl) -> bool {
    bound_bool(view, AUTO_HIDE_PROPERTY) == Some(true)
}

/// The state property a box or track shows: pressed, else hovered, else resting, each falling
/// back to the next less specific state the control names.
pub(crate) fn shown_state(control: &ResolvedControl, hovered: bool, pressed: bool) -> &'static str {
    let named = |property: &str| {
        control
            .properties
            .get(property)
            .and_then(Value::as_str)
            .is_some_and(|name| !name.is_empty())
    };
    let wanted = usize::from(!pressed) + usize::from(!pressed && !hovered);
    STATE_PROPERTIES[wanted..]
        .iter()
        .copied()
        .find(|property| named(property))
        .unwrap_or(DEFAULT_STATE)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A box lengthened by a declared minimum still spans the whole range over its own travel.
    #[test]
    fn a_declared_minimum_keeps_the_dragged_box_under_the_pointer() {
        let metrics = crate::ScrollMetrics {
            content: 200.0,
            viewport: 50.0,
            track: Some([0.0, 0.0, 4.0, 100.0]),
            thumb: Some([0.0, 0.0, 4.0, 40.0]),
            thumb_travel_drag: true,
            ..Default::default()
        };
        assert_eq!(metrics.thumb_drag_target(30.0), 75.0);
        assert_eq!(metrics.thumb_drag_target(60.0), 150.0);
        let undeclared = crate::ScrollMetrics {
            thumb_travel_drag: false,
            ..metrics
        };
        assert_eq!(undeclared.thumb_drag_target(30.0), 60.0);
    }

    fn control(states: &[&str]) -> ResolvedControl {
        ResolvedControl {
            name: "box".into(),
            control_type: Some("scrollbar_box".into()),
            base: None,
            unresolved_base: None,
            properties: states
                .iter()
                .map(|state| ((*state).to_owned(), Value::String(format!("{state}_child"))))
                .collect::<std::collections::BTreeMap<_, _>>()
                .into(),
            children: Vec::new(),
            factory: None,
        }
    }

    /// Each state falls back to the next less specific one the control names.
    #[test]
    fn missing_states_fall_back_to_less_specific_ones() {
        let full = control(&STATE_PROPERTIES);
        assert_eq!(shown_state(&full, false, false), DEFAULT_STATE);
        assert_eq!(shown_state(&full, true, false), HOVER_STATE);
        assert_eq!(shown_state(&full, true, true), PRESSED_STATE);
        assert_eq!(shown_state(&full, false, true), PRESSED_STATE);
        let hover_only = control(&[HOVER_STATE, DEFAULT_STATE]);
        assert_eq!(shown_state(&hover_only, true, true), HOVER_STATE);
        let resting = control(&[DEFAULT_STATE]);
        assert_eq!(shown_state(&resting, true, true), DEFAULT_STATE);
    }

    /// Declared values clamp into their bounds and absent ones keep the defaults.
    #[test]
    fn declared_values_clamp_into_their_bounds() {
        let mut view = control(&[]);
        assert_eq!(min_thumb(&view), DEFAULT_MIN_THUMB);
        assert_eq!(wheel_seconds(&view), None);
        assert!(!auto_hide(&view));
        let properties = std::collections::BTreeMap::from([
            (MIN_THUMB_PROPERTY.to_owned(), Value::from(3.0)),
            (WHEEL_SECONDS_PROPERTY.to_owned(), Value::from(-1.0)),
            (AUTO_HIDE_PROPERTY.to_owned(), Value::Bool(true)),
        ]);
        view.properties = properties.into();
        assert_eq!(min_thumb(&view), 1.0);
        assert_eq!(wheel_seconds(&view), Some(0.0));
        assert!(auto_hide(&view));
    }
}
