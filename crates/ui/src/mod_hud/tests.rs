#[cfg(test)]
mod tests {
    use super::*;
    /// A draft holds many independent widgets beyond the legacy sixteen.
    #[test]
    fn huds_accept_many_small_cards() {
        let cards: Vec<_> = (0..24)
            .map(|index| serde_json::json!({"id":format!("module_{index}")}))
            .collect();
        let hud: Hud = serde_json::from_value(serde_json::json!({"cards":cards})).unwrap();
        assert!(hud.validate().is_ok());
    }
    #[test]
    fn larger_card_catalog_keeps_explicit_bounds() {
        let mut hud: Hud = serde_json::from_value(serde_json::json!({"cards":(0..MAX_HUD_CARDS)
            .map(|index|serde_json::json!({"id":format!("card_{index}")})).collect::<Vec<_>>()}))
        .unwrap();
        assert!(hud.validate().is_ok());
        let mut excess = hud.cards[0].clone();
        excess.id = "excess".into();
        hud.cards.push(excess);
        assert!(hud.validate().is_err());
    }
    #[test]
    fn card_defaults_preserve_legacy_background_and_validate_new_presentation_fields() {
        let mut hud: Hud = serde_json::from_str(r#"{"cards":[{"id":"supplies"}]}"#).unwrap();
        assert!(hud.cards[0].title.is_empty());
        assert_eq!(hud.cards[0].position, None);
        assert_eq!(hud.cards[0].background_opacity, 0.82);
        assert_eq!(hud.cards[0].width, DEFAULT_CARD_WIDTH);
        assert!(hud.validate().is_ok());
        for opacity in [0., 0.35, 1.] {
            hud.cards[0].background_opacity = opacity;
            assert!(hud.validate().is_ok());
        }
        for opacity in [f32::NAN, -0.01, 1.01] {
            hud.cards[0].background_opacity = opacity;
            assert!(hud.validate().is_err());
        }
        hud.cards[0].background_opacity = 0.82;
        for position in [[f32::NAN, 0.], [-0.01, 0.], [1.01, 0.], [0., f32::INFINITY]] {
            hud.cards[0].position = Some(position);
            assert!(hud.validate().is_err());
        }
        for position in [[0., 0.], [0.5, 0.5], [1., 1.]] {
            hud.cards[0].position = Some(position);
            assert!(hud.validate().is_ok());
        }
        hud.cards[0].editor_label = "Bad\nlabel".into();
        assert!(hud.validate().is_err());
    }
    #[test]
    fn card_width_is_bounded() {
        let mut hud: Hud = serde_json::from_str(r#"{"cards":[{"id":"fixture"}]}"#).unwrap();
        for width in [1., 64., MAX_CARD_WIDTH] {
            hud.cards[0].width = width;
            assert!(hud.validate().is_ok(), "width={width}");
        }
        for width in [f32::NAN, f32::INFINITY, 0.99, MAX_CARD_WIDTH + 1.] {
            hud.cards[0].width = width;
            assert!(hud.validate().is_err(), "width={width}");
        }
    }
    #[test]
    fn editor_results_are_bounded_and_cancellation_carries_no_changes() {
        assert!(EditorResult::default().validate().is_ok());
        let saved = EditorResult {
            hidden_cards: Vec::new(),
            replace_pending_reset: false,
            saved: true,
            reset: false,
            placements: vec![Placement {
                id: "equipment".into(),
                anchor: Anchor::Center,
                offset: [0.; 2],
                scale: 1.,
                position: Some([0.5, 0.5]),
            }],
        };
        assert!(saved.validate().is_ok());
        let mut malformed_offset = saved.clone();
        malformed_offset.placements[0].offset = [f32::NAN, 0.];
        assert!(malformed_offset.validate().is_err());
        for scale in [f32::NAN, 0.24, 5.01] {
            let mut malformed = saved.clone();
            malformed.placements[0].scale = scale;
            assert!(malformed.validate().is_err());
        }
        assert!(
            EditorResult {
                saved: false,
                ..saved.clone()
            }
            .validate()
            .is_err()
        );
        let mut duplicate = saved.clone();
        duplicate.placements.push(duplicate.placements[0].clone());
        assert!(duplicate.validate().is_err());
        let mut excess = saved;
        excess.placements[0].position = Some([1.01, 0.]);
        assert!(excess.validate().is_err());
    }
    #[test]
    fn editor_autosave_is_opt_in_and_custom_actions_remain_bounded() {
        let mut hud: Hud = serde_json::from_str(r#"{"cards":[]}"#).unwrap();
        assert!(!hud.autosave && hud.surface.is_none());
        let done_last = format!("hud.done:{}", crate::mod_panel::MAX_PANEL_CONTROLS - 1);
        let done_excess = format!("hud.done:{}", crate::mod_panel::MAX_PANEL_CONTROLS);
        let card_last = format!("hud.card:{}", MAX_HUD_CARDS - 1);
        let card_excess = format!("hud.card:{}", MAX_HUD_CARDS);
        for (action, valid) in [
            ("hud.close", true),
            ("hud.done:0", true),
            (done_last.as_str(), true),
            (done_excess.as_str(), false),
            (card_last.as_str(), true),
            (card_excess.as_str(), false),
            ("mod.control:0", false),
            ("button.resume_game", false),
        ] {
            hud.surface = Some(crate::mod_panel::Surface {
                collections: Default::default(),
                transitions: Default::default(),
                screen: "fixture.editor".into(),
                document: serde_json::json!({"namespace":"fixture","editor":{
                    "type":"button","button_mappings":[{"from_button_id":"button.menu_select",
                        "to_button_id":action,"mapping_type":"pressed"}]
                }})
                .to_string(),
                bindings: Default::default(),
            });
            assert_eq!(hud.validate().is_ok(), valid, "route {action}");
        }
    }
    #[test]
    fn hud_rejects_ambiguous_ids_and_formatted_titles() {
        let card = Card {
            id: "armor".into(),
            title: "Armor".into(),
            anchor: Anchor::TopRight,
            offset: [-6., 6.],
            scale: 1.,
            ..Default::default()
        };
        let mut hud = Hud {
            cards: vec![card],
            ..Default::default()
        };
        assert!(hud.validate().is_ok());
        hud.cards.push(hud.cards[0].clone());
        assert!(hud.validate().is_err());
        hud.cards.pop();
        hud.cards[0].title = "§khidden".into();
        assert!(hud.validate().is_err());
    }
}
use super::*;

/// Native cards, factory resets and editor results accept the full published HUD scale range.
#[test]
fn uniform_card_scale_supports_quarter_to_five() {
    for scale in [super::MIN_CARD_SCALE, super::MAX_CARD_SCALE] {
        let hud = super::Hud {
            cards: vec![super::Card {
                id: "clock".into(),
                scale,
                reset_scale: Some(scale),
                ..Default::default()
            }],
            ..Default::default()
        };
        hud.validate().unwrap();
        super::EditorResult {
            hidden_cards: Vec::new(),
            replace_pending_reset: false,
            saved: true,
            reset: false,
            placements: vec![super::Placement {
                id: "clock".into(),
                anchor: super::Anchor::TopLeft,
                offset: [0.; 2],
                position: None,
                scale,
            }],
        }
        .validate()
        .unwrap();
    }
}

/// Studio's fixed action names use the same bounds as cards, controls and anchor choices.
#[test]
fn studio_surface_actions_accept_exact_names_and_reject_out_of_range_indices() {
    use crate::mod_panel::{MAX_PANEL_CONTROLS, Surface};
    let check = |action: &str| {
        let hud = super::Hud {
            surface: Some(Surface {
                screen: "studio.root".into(),
                document: serde_json::json!({"namespace":"studio","root":{"type":"screen","controls":[{"action":{
                    "type":"button", "size":[20,20], "button_mappings":[{"from_button_id":"button.menu_select","to_button_id":action,"mapping_type":"pressed"}]
                }}]}}).to_string(),
                bindings: Default::default(), transitions: Default::default(), collections: Default::default(),            }),
            ..Default::default()
        };
        hud.validate()
    };
    for action in [
        "hud.undo",
        "hud.redo",
        "hud.scale",
        "hud.scale_down",
        "hud.scale_up",
        "hud.save",
        "hud.cancel",
        "hud.close",
        "hud.reset",
        "hud.grid",
        "hud.snap",
    ] {
        check(action).unwrap();
    }
    for (prefix, bound) in [
        ("hud.visibility:", super::MAX_HUD_CARDS),
        ("hud.select:", super::MAX_HUD_CARDS),
        ("hud.card:", super::MAX_HUD_CARDS),
        ("hud.anchor:", 9),
        ("hud.control:", MAX_PANEL_CONTROLS),
        ("hud.done:", MAX_PANEL_CONTROLS),
    ] {
        for index in [0, bound - 1] {
            check(&format!("{prefix}{index}")).unwrap();
        }
        for suffix in [
            bound.to_string(),
            "-1".into(),
            "1.5".into(),
            "".into(),
            "99999999999999999999999999999".into(),
        ] {
            assert!(check(&format!("{prefix}{suffix}")).is_err());
        }
    }
    assert!(check("hud.toggle:0").is_err());
}

#[test]
fn amendment_visibility_and_inspector_fields_are_additive_and_validated() {
    let legacy: super::Hud = serde_json::from_value(serde_json::json!({
        "cards":[{"id":"fps"}]
    }))
    .unwrap();
    assert!(legacy.cards[0].visible);
    assert!(super::Card::default().visible);
    assert_eq!(legacy.inspector_size, None);
    let mut hud: super::Hud = serde_json::from_value(serde_json::json!({
        "cards":[{"id":"fps","visible":false}],
        "inspector_size":[180,240]
    }))
    .unwrap();
    hud.validate().unwrap();
    assert!(!hud.cards[0].visible);
    assert_eq!(
        serde_json::from_value::<super::Hud>(serde_json::to_value(&hud).unwrap()).unwrap(),
        hud
    );
    for value in [0., -1., f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        for axis in 0..2 {
            let mut size = [180., 240.];
            size[axis] = value;
            hud.inspector_size = Some(size);
            assert!(hud.validate().unwrap_err().contains("inspector size"));
        }
    }
    for value in [
        serde_json::json!([12]),
        serde_json::json!([12, 24, 36]),
        serde_json::json!([12, "24"]),
    ] {
        assert!(
            serde_json::from_value::<super::Hud>(
                serde_json::json!({"cards":[],"inspector_size":value})
            )
            .is_err()
        );
    }
    assert!(
        serde_json::from_value::<super::Hud>(
            serde_json::json!({"cards":[{"id":"fps","visible":0}]})
        )
        .is_err()
    );
}

#[test]
fn studio_hidden_results_require_unique_saved_placement_ids() {
    let mut result = super::EditorResult {
        saved: true,
        placements: vec![super::Placement {
            id: "fps".into(),
            anchor: super::Anchor::TopLeft,
            offset: [0.; 2],
            scale: 1.,
            position: None,
        }],
        hidden_cards: vec!["fps".into()],
        ..Default::default()
    };
    result.validate().unwrap();
    result.hidden_cards.push("fps".into());
    assert!(result.validate().is_err());
    result.hidden_cards = vec!["unknown".into()];
    assert!(result.validate().is_err());
    result.hidden_cards = vec!["fps".into()];
    result.saved = false;
    assert!(result.validate().is_err());
}

#[test]
fn studio_cancellation_cannot_replace_pending_reset_intent() {
    let result = super::EditorResult {
        replace_pending_reset: true,
        ..Default::default()
    };
    assert!(result.validate().is_err());
}

/// Background radii default to the original look and reject non-finite or out-of-range values.
#[test]
fn background_radii_default_to_the_original_look_and_are_bounded() {
    let mut hud: super::Hud = serde_json::from_value(serde_json::json!({
        "cards":[{"id":"fps"}]
    }))
    .unwrap();
    assert_eq!(
        hud.cards[0].background_radius,
        super::DEFAULT_CARD_BACKGROUND_RADIUS
    );
    hud.validate().unwrap();
    for radius in [0., 8., super::MAX_CARD_BACKGROUND_RADIUS] {
        hud.cards[0].background_radius = radius;
        hud.validate().unwrap();
    }
    for radius in [
        -0.5,
        super::MAX_CARD_BACKGROUND_RADIUS + 0.5,
        f32::NAN,
        f32::INFINITY,
    ] {
        let mut card = hud.clone();
        card.cards[0].background_radius = radius;
        assert!(card.validate().is_err(), "card radius {radius}");
    }
}

/// Trusted host preview sizes are validated but never cross JSON.
#[test]
fn host_preview_size_is_validated_and_not_serialized() {
    let mut hud = Hud {
        cards: vec![Card {
            id: "touch".into(),
            width: 1.,
            editor_size: Some([2048., 1600.]),
            ..Default::default()
        }],
        ..Default::default()
    };
    assert!(hud.validate().is_ok());
    let encoded = serde_json::to_value(&hud).unwrap();
    assert!(encoded["cards"][0].get("editor_size").is_none());
    let decoded: Hud = serde_json::from_value(encoded).unwrap();
    assert_eq!(decoded.cards[0].editor_size, None);
    for size in [[f32::NAN, 1.], [1., f32::INFINITY], [0., 1.], [-1., 1.]] {
        hud.cards[0].editor_size = Some(size);
        assert!(hud.validate().is_err());
    }
}
