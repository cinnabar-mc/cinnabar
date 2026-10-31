//! Restored controls require a new explicit choice and never reuse retired saved keys.

use super::*;
use render_api::far_terrain::{
    DeviceTier, FULL_DETAIL_AUTO, FarTerrainMode, MAX_FULL_DETAIL_CHUNKS,
};

/// Resolves the new persisted contract without relying on registry positions.
fn control(name: &str) -> SettingId {
    SettingId::from_name(name).expect("restored far-terrain control")
}

#[test]
fn far_terrain_controls_require_explicit_opt_in() {
    let far = control("far_terrain_mode");
    let full = control("far_terrain_full_detail_distance");
    let mut options = SettingsOptions::default();
    options.set(SettingId::RenderDistance.index(), 64);
    assert_eq!(
        options.user_settings().video.far_terrain,
        FarTerrainMode::Off
    );
    assert_eq!(
        options.user_settings().video.far_terrain,
        ui::VideoSettings::default().far_terrain
    );
    assert_eq!(options.value(full) as u32, FULL_DETAIL_AUTO);
    assert!(options.set(far.index(), FarTerrainMode::On.index()));
    assert!(
        options
            .user_settings()
            .video
            .far_terrain
            .enabled(64, DeviceTier::High)
    );
}

#[test]
fn far_terrain_current_choices_survive_save_and_video_reset() {
    let far = control("far_terrain_mode");
    let full = control("far_terrain_full_detail_distance");
    for mode in FarTerrainMode::ALL {
        let mut options = SettingsOptions::default();
        options.set(far.index(), mode.index());
        options.set(full.index(), MAX_FULL_DETAIL_CHUNKS as i32);
        let mut loaded = SettingsOptions::decode(&serde_json::to_vec(&options).unwrap()).unwrap();
        assert_eq!(loaded.user_settings().video.far_terrain, mode);
        assert_eq!(
            loaded.user_settings().video.full_detail_distance_chunks,
            MAX_FULL_DETAIL_CHUNKS
        );
        loaded.reset_group(SettingsGroup::Video);
        assert_eq!(
            loaded.user_settings().video.far_terrain,
            FarTerrainMode::Off
        );
        assert_eq!(
            loaded.user_settings().video.full_detail_distance_chunks,
            FULL_DETAIL_AUTO
        );
    }
}

/// Applying a quality preset cannot opt the player in or discard an explicit choice.
#[test]
fn far_terrain_video_presets_preserve_the_player_choice() {
    let far = control("far_terrain_mode");
    for mode in FarTerrainMode::ALL {
        for preset in video_presets::VideoPreset::ALL {
            let mut options = SettingsOptions::default();
            options.set(far.index(), mode.index());
            options.apply_video_preset(preset);
            assert_eq!(options.user_settings().video.far_terrain, mode);
        }
    }
}

#[test]
fn far_terrain_legacy_keys_never_reenable_or_restore_detail() {
    for schema in [1, persistence::SETTINGS_SCHEMA] {
        for old in [0, 1, 2, i32::MIN, i32::MAX] {
            let saved = serde_json::json!({"schema": schema, "values": {
                "far_terrain": old, "full_detail_distance": 16
            }});
            let options = SettingsOptions::decode(&serde_json::to_vec(&saved).unwrap()).unwrap();
            let video = options.user_settings().video;
            assert_eq!(video.far_terrain, FarTerrainMode::Off);
            assert_eq!(video.full_detail_distance_chunks, FULL_DETAIL_AUTO);
        }
    }
}

#[test]
fn far_terrain_invalid_saved_mode_stays_off() {
    for value in [i32::MIN, -1, FarTerrainMode::ALL.len() as i32, i32::MAX] {
        let saved = serde_json::json!({"values": {"far_terrain_mode": value}});
        let options = SettingsOptions::decode(&serde_json::to_vec(&saved).unwrap()).unwrap();
        assert_eq!(
            options.user_settings().video.far_terrain,
            FarTerrainMode::Off
        );
    }
}
