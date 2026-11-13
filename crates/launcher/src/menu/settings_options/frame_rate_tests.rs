use render_api::FrameRateLimit::{self, Fixed, Unlimited};

use super::{
    DEFAULT_FRAME_RATE, FRAME_RATE_UNLIMITED, MAX_FIXED_FRAME_RATE, MIN_FIXED_FRAME_RATE,
    SETTINGS_SCHEMA, SettingId, SettingsGroup, SettingsOptions, frame_rate_label, frame_rate_limit,
    frame_rate_value,
};

/// Returns the frame-rate limit applied by the loaded settings.
fn limit(settings: &SettingsOptions) -> FrameRateLimit {
    settings.user_settings().video.frame_rate_limit
}

/// The frame-rate slider value written to the settings file, if any.
fn stored(settings: &SettingsOptions) -> Option<serde_json::Value> {
    serde_json::to_value(settings).unwrap()["values"]
        .get(SettingId::MaxFramerate.name())
        .cloned()
}

fn round_trip(settings: &SettingsOptions) -> SettingsOptions {
    SettingsOptions::decode(&serde_json::to_vec(settings).unwrap()).unwrap()
}

/// A slider stop other than this platform's default.
fn non_default_stop() -> i32 {
    if DEFAULT_FRAME_RATE == FRAME_RATE_UNLIMITED {
        60
    } else {
        FRAME_RATE_UNLIMITED
    }
}

/// The slider starts at the shared platform default: 60 FPS on phones, Unlimited on desktop.
#[test]
fn the_slider_default_is_the_platform_default() {
    let option = SettingId::MaxFramerate.definition();
    assert_eq!(option.default, DEFAULT_FRAME_RATE);
    assert_eq!(frame_rate_limit(option.default), FrameRateLimit::DEFAULT);
    assert_eq!(frame_rate_value(FrameRateLimit::platform_default(true)), 60);
    assert_eq!(
        frame_rate_value(FrameRateLimit::platform_default(false)),
        FRAME_RATE_UNLIMITED
    );
    for mobile in [true, false] {
        let default = FrameRateLimit::platform_default(mobile);
        assert_eq!(frame_rate_limit(frame_rate_value(default)), default);
    }
}

/// Untouched settings never write the frame-rate default, so files saved before a platform's
/// default changed pick up the new default without a migration.
#[test]
fn an_untouched_frame_rate_follows_the_current_default() {
    let mut settings = SettingsOptions::default();
    assert_eq!(stored(&settings), None);
    assert!(!settings.set(SettingId::MaxFramerate.index(), DEFAULT_FRAME_RATE));
    assert_eq!(stored(&settings), None);
    for saved in [
        serde_json::json!({ "values": {} }),
        serde_json::json!({ "schema": SETTINGS_SCHEMA, "values": { "field_of_view": 70 } }),
    ] {
        let loaded = SettingsOptions::decode(&serde_json::to_vec(&saved).unwrap()).unwrap();
        assert_eq!(limit(&loaded), FrameRateLimit::DEFAULT, "{saved}");
        assert_eq!(stored(&loaded), None, "{saved}");
    }
}

/// A saved choice, including Unlimited, stays in the file and keeps applying whatever the
/// platform default is.
#[test]
fn explicit_frame_rate_choices_survive_a_default_change() {
    for value in [30, 60, 120, MAX_FIXED_FRAME_RATE, FRAME_RATE_UNLIMITED] {
        let saved = serde_json::json!({
            "schema": SETTINGS_SCHEMA, "values": { "max_framerate": value }
        });
        let loaded = SettingsOptions::decode(&serde_json::to_vec(&saved).unwrap()).unwrap();
        assert_eq!(limit(&loaded), frame_rate_limit(value), "{saved}");
        let reloaded = round_trip(&loaded);
        assert_eq!(stored(&reloaded), Some(value.into()), "{saved}");
        assert_eq!(limit(&reloaded), frame_rate_limit(value), "{saved}");
    }
    let mut settings = SettingsOptions::default();
    assert!(settings.set(SettingId::MaxFramerate.index(), non_default_stop()));
    assert_eq!(
        stored(&round_trip(&settings)),
        Some(non_default_stop().into())
    );
}

/// Missing and unsupported FPS values use the default without becoming a one-FPS cap.
#[test]
fn frame_rate_missing_and_invalid_values_use_the_default() {
    assert_eq!(limit(&SettingsOptions::default()), FrameRateLimit::DEFAULT);
    for schema in [None, Some(SETTINGS_SCHEMA)] {
        for value in [None, Some(0), Some(-1), Some(i32::MIN), Some(i32::MAX)] {
            let mut saved = serde_json::json!({ "values": {} });
            if let Some(schema) = schema {
                saved["schema"] = schema.into();
            }
            if let Some(value) = value {
                saved["values"]["max_framerate"] = value.into();
            }
            let settings = SettingsOptions::decode(&serde_json::to_vec(&saved).unwrap()).unwrap();
            assert_eq!(limit(&settings), FrameRateLimit::DEFAULT, "{saved}");
            assert_eq!(
                settings.value(SettingId::MaxFramerate),
                DEFAULT_FRAME_RATE,
                "{saved}"
            );
            assert_eq!(
                limit(&round_trip(&settings)),
                FrameRateLimit::DEFAULT,
                "{saved}"
            );
        }
    }
}

#[test]
fn choosing_unlimited_survives_saves_and_video_reset_restores_the_default() {
    let mut settings = SettingsOptions::default();
    assert!(settings.set(SettingId::MaxFramerate.index(), MIN_FIXED_FRAME_RATE));
    assert!(settings.set(SettingId::MaxFramerate.index(), FRAME_RATE_UNLIMITED));
    assert_eq!(limit(&settings), Unlimited);
    let mut loaded = round_trip(&settings);
    assert_eq!(limit(&loaded), Unlimited);
    loaded.set(SettingId::MaxFramerate.index(), MIN_FIXED_FRAME_RATE);
    loaded.reset_group(SettingsGroup::Video);
    assert_eq!(limit(&loaded), FrameRateLimit::DEFAULT);
    assert_eq!(stored(&loaded), None);
}

#[test]
fn frame_rate_caption_names_caps_and_unlimited() {
    let translate = |key: &str| format!("<{key}>");
    assert_eq!(frame_rate_label(144, &translate), "144");
    for value in [-1, 0, FRAME_RATE_UNLIMITED, i32::MAX] {
        assert_eq!(
            frame_rate_label(value, &translate),
            "<options.framerateLimit.max>"
        );
    }
}

/// Every slider stop applies the selected limit and survives a settings round trip.
#[test]
fn frame_rate_slider_stops_cover_every_cap_and_unlimited() {
    let index = SettingId::MaxFramerate.index();
    let option = SettingId::MaxFramerate.definition();
    assert_eq!(option.min, MIN_FIXED_FRAME_RATE);
    assert_eq!(frame_rate_limit(option.max), Unlimited);
    for fps in MIN_FIXED_FRAME_RATE..=FRAME_RATE_UNLIMITED {
        let expected = if fps <= MAX_FIXED_FRAME_RATE {
            Fixed(u16::try_from(fps).unwrap().try_into().unwrap())
        } else {
            Unlimited
        };
        assert_eq!(frame_rate_value(expected), fps);
        let mut settings = SettingsOptions::default();
        settings.set(index, fps);
        assert_eq!(limit(&settings), expected);
        assert_eq!(limit(&round_trip(&settings)), expected);
        settings.reset_group(SettingsGroup::Video);
        assert_eq!(limit(&settings), FrameRateLimit::DEFAULT);
    }
}
