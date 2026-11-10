//! Authored scrollbar extension bounds: a scroll view's minimum box fraction, wheel easing and
//! auto-hide, and the state children its box and track name. The engine clamps any value a
//! binding supplies later.

use json_ui::scrollbar::{
    AUTO_HIDE_PROPERTY, MAX_WHEEL_SECONDS, MIN_THUMB_PROPERTY, STATE_PROPERTIES, STATE_TYPES,
    WHEEL_SECONDS_PROPERTY, accepts_min_thumb, accepts_wheel_seconds,
};
use serde_json::{Map, Value};

/// Rejects a malformed or out-of-range scrollbar property on a control of `kind`.
pub(super) fn validate(kind: &str, object: &Map<String, Value>) -> Result<(), String> {
    let number = |name: &str| object.get(name).map(|value| value.as_f64());
    if let Some(value) = number(MIN_THUMB_PROPERTY)
        && !value.is_some_and(accepts_min_thumb)
    {
        return Err(format!(
            "surface {MIN_THUMB_PROPERTY} must be a number from 0 to 1"
        ));
    }
    if let Some(value) = number(WHEEL_SECONDS_PROPERTY)
        && !value.is_some_and(accepts_wheel_seconds)
    {
        return Err(format!(
            "surface {WHEEL_SECONDS_PROPERTY} must be a number from 0 to {MAX_WHEEL_SECONDS}"
        ));
    }
    if object
        .get(AUTO_HIDE_PROPERTY)
        .is_some_and(|value| !value.is_boolean())
    {
        return Err(format!("surface {AUTO_HIDE_PROPERTY} must be a boolean"));
    }
    if kind != "scroll_view"
        && [
            MIN_THUMB_PROPERTY,
            WHEEL_SECONDS_PROPERTY,
            AUTO_HIDE_PROPERTY,
        ]
        .iter()
        .any(|name| object.contains_key(*name))
    {
        return Err("surface scrollbar settings belong on a scroll_view".into());
    }
    for name in STATE_PROPERTIES {
        let Some(value) = object.get(name) else {
            continue;
        };
        if !STATE_TYPES.contains(&kind) {
            return Err(format!(
                "surface {name} belongs on a scrollbar_box or scroll_track"
            ));
        }
        if !value.as_str().is_some_and(|child| super::identifier(child)) {
            return Err(format!("surface {name} must name a child control"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::Surface;
    use serde_json::{Value, json};

    /// A surface with one scroll view whose view, box and track carry the given properties.
    fn surface(view: Value, thumb: Value, track: Value) -> Surface {
        let merge = |mut base: Value, extra: Value| {
            base.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            base
        };
        let fill = json!({"type":"image","size":["100%","100%"]});
        let thumb = merge(
            json!({"type":"scrollbar_box","size":[4,10],"draggable":"vertical",
                "controls":[{"idle":fill.clone()},{"lit":fill.clone()},{"held":fill.clone()}]}),
            thumb,
        );
        let track = merge(
            json!({"type":"scroll_track","size":[4,40],"controls":[{"idle":fill.clone()}]}),
            track,
        );
        let view = merge(
            json!({"type":"scroll_view","size":[40,40],"scroll_content":"content",
                "scroll_view_port":"port","scrollbar_track":"track","scrollbar_box":"thumb",
                "scroll_box_and_track_panel":"bar","controls":[
                    {"port":{"type":"panel","controls":[{"content":{"type":"panel","size":[36,90]}}]}},
                    {"bar":{"type":"panel","size":[4,40],"controls":[{"track":track},{"thumb":thumb}]}}]}),
            view,
        );
        Surface {
            screen: "bars.root".into(),
            document:
                json!({"namespace":"bars","root":{"type":"panel","controls":[{"list":view}]}})
                    .to_string(),
            bindings: Default::default(),
            transitions: Default::default(),
            collections: Default::default(),
        }
    }

    #[test]
    fn bounded_scrollbar_settings_and_named_states_are_admitted() {
        let valid = surface(
            json!({"cinnabar_min_thumb_fraction":0.25,"cinnabar_scroll_seconds":0,
                "cinnabar_scrollbar_auto_hide":true}),
            json!({"cinnabar_default_control":"idle","cinnabar_hover_control":"lit",
                "cinnabar_pressed_control":"held"}),
            json!({"cinnabar_default_control":"idle"}),
        );
        valid.validate().unwrap();
        valid.validate_hud().unwrap();
        surface(json!({"cinnabar_scroll_seconds":1}), json!({}), json!({}))
            .validate()
            .unwrap();
    }

    #[test]
    fn malformed_or_misplaced_scrollbar_settings_are_rejected() {
        for (view, thumb, track) in [
            (
                json!({"cinnabar_min_thumb_fraction":1.5}),
                json!({}),
                json!({}),
            ),
            (
                json!({"cinnabar_min_thumb_fraction":-0.1}),
                json!({}),
                json!({}),
            ),
            (
                json!({"cinnabar_scroll_seconds":1.01}),
                json!({}),
                json!({}),
            ),
            (
                json!({"cinnabar_scroll_seconds":"fast"}),
                json!({}),
                json!({}),
            ),
            (
                json!({"cinnabar_scrollbar_auto_hide":1}),
                json!({}),
                json!({}),
            ),
            (
                json!({}),
                json!({"cinnabar_scrollbar_auto_hide":true}),
                json!({}),
            ),
            (json!({}), json!({"cinnabar_hover_control":7}), json!({})),
            (json!({}), json!({}), json!({"cinnabar_pressed_control":""})),
            (
                json!({"cinnabar_hover_control":"port"}),
                json!({}),
                json!({}),
            ),
        ] {
            let case = format!("{view} {thumb} {track}");
            assert!(surface(view, thumb, track).validate().is_err(), "{case}");
        }
    }
}
