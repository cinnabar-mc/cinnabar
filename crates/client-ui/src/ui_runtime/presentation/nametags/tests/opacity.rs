use super::*;

#[test]
fn menu_preference_updates_nametag_scene_while_menu_is_closed() {
    use launcher::menu::settings_options::{SettingId, SettingsOptions};
    let mut runtime =
        super::super::super::UiPresentationRuntime::new(ui_engine::test_support::fixture_font())
            .unwrap();
    runtime.set_nametag_anchors(vec![anchor("A")]);
    let mut settings = SettingsOptions::default();
    for percent in [0, 50, 100] {
        settings.set(SettingId::NametagBackgroundOpacity.index(), percent);
        runtime.set_chat_settings_snapshot((Arc::new(settings.clone()), None));
        let painted = runtime.nametag_scene();
        assert!(!painted.records.is_empty());
        for record in &painted.records {
            assert_eq!(
                record.color[3],
                if record.uv[2] < 0.0 {
                    percent as f32 / 100.0
                } else {
                    1.0
                }
            );
        }
    }
}

#[test]
fn background_percentage_changes_only_plates_and_preserves_mod_overrides() {
    for sneaking in [false, true] {
        for percent in [0, 25, 50, 100] {
            let mut tag = anchor("AB\nA");
            tag.depth_tested = sneaking;
            tag.sneaking = sneaking;
            tag.text_alpha = if sneaking { DIM_TEXT_ALPHA } else { 1.0 };
            let mut painted = scene(&[tag]);
            let original = painted.records.clone();
            super::super::style::background_opacity(&mut painted, percent as f32 / 100.0);
            for (record, before) in painted.records.iter().zip(&original) {
                if record.uv[2] < 0.0 {
                    assert_eq!(record.color[3], percent as f32 / 100.0);
                    assert_eq!(record.fragment_color([1.0; 4]).is_some(), percent > 0);
                    assert_eq!(record.rect, before.rect);
                    assert_eq!(record.depth_tested, before.depth_tested);
                } else {
                    assert_eq!(record, before);
                }
            }
            let override_color = [0.2, 0.3, 0.4, 0.75];
            style_scene(
                &mut painted,
                &ui::mod_presentation::NametagStyle {
                    background_color: Some(override_color),
                    ..Default::default()
                },
                None,
            );
            assert!(
                painted
                    .records
                    .iter()
                    .filter(|record| record.uv[2] < 0.0)
                    .all(|record| record.color == override_color)
            );
        }
    }
}

/// Replacement removes a formatted tint, while opacity still preserves native sneaking alpha.
#[test]
fn replacement_and_default_text_opacity_preserve_native_coverage() {
    let glyph = NametagRecord {
        anchor: [1., 2., 3.],
        runtime_id: 11,
        color: [0., 1., 0., 0.125],
        uv: [0., 0., 1., 1.],
        ..Default::default()
    };
    let remote = NametagRecord {
        runtime_id: 12,
        ..glyph
    };
    let mut scene = NametagScene {
        records: vec![glyph, remote],
        ..Default::default()
    };
    let style = ui::mod_presentation::NametagStyle {
        opacity: 0.5,
        own_text_color: Some([1., 0., 0., 1.]),
        ..Default::default()
    };
    style_scene(&mut scene, &style, Some(glyph.runtime_id));
    assert_eq!(scene.records[0].color, [1., 0., 0., 0.0625]);
    assert_eq!(scene.records[1].color, [0., 1., 0., 0.0625]);
    assert_eq!(
        scene.records[0].fragment_color([1.; 4]),
        Some([1., 0., 0., 0.0625])
    );
    assert_eq!(
        scene.records[1].fragment_color([1.; 4]),
        Some([0., 1., 0., 0.0625])
    );
    assert!(
        scene.records[0]
            .fragment_color([1., 1., 1., 0.05])
            .is_none()
    );
}

/// Zero opacity discards glyphs and plates; visible glyphs retain atlas coverage and native alpha.
#[test]
fn faded_sneak_and_through_wall_glyphs_keep_their_atlas_coverage() {
    for sneaking in [false, true] {
        for opacity in [0.0, 0.5, 1.0] {
            let mut label = anchor("A");
            label.depth_tested = sneaking;
            label.sneaking = sneaking;
            label.text_alpha = if sneaking { DIM_TEXT_ALPHA } else { 1.0 };
            let mut scene = scene(&[label]);
            let style = ui::mod_presentation::NametagStyle {
                opacity,
                text_color: Some([1.0, 1.0, 1.0, 0.25]),
                ..Default::default()
            };
            style_scene(&mut scene, &style, None);
            for record in scene.records.iter().filter(|record| record.text != 0) {
                let native_alpha = if sneaking { DIM_TEXT_ALPHA } else { 1.0 };
                let fragment = record.fragment_color([1.0, 1.0, 1.0, 0.5]);
                if opacity == 0.0 {
                    assert!(fragment.is_none());
                } else {
                    assert!(
                        (fragment.unwrap()[3] - native_alpha * opacity * 0.25 * 0.5).abs() < 1e-6
                    );
                }
                assert!(
                    record
                        .fragment_color([1.0, 1.0, 1.0, render_model::NAMETAG_ALPHA_CUTOFF])
                        .is_none()
                );
            }
            // Visible plates ignore atlas contents; zero opacity discards them.
            assert_eq!(
                scene.records[0].fragment_color([0.0; 4]).is_some(),
                opacity > 0.0
            );
        }
    }
}

/// Coincident actors retain independent cosmetic ownership after label sorting.
#[test]
fn coincident_labels_use_runtime_identity_for_own_text_colour() {
    let mut own = anchor("A");
    own.runtime_id = 11;
    let mut remote = own.clone();
    remote.runtime_id = 12;
    let mut scene = scene(&[remote, own]);
    let style = ui::mod_presentation::NametagStyle {
        own_text_color: Some([1.0, 0.0, 0.0, 1.0]),
        ..Default::default()
    };
    style_scene(&mut scene, &style, Some(11));
    for record in scene.records.iter().filter(|record| record.text != 0) {
        assert_eq!(
            record.color[..3],
            if record.runtime_id == 11 {
                [1.0, 0.0, 0.0]
            } else {
                [1.0; 3]
            }
        );
    }
}
