use super::*;

#[test]
fn login_profanity_preference_comes_from_saved_settings_with_a_hidden_menu() {
    use launcher::menu::settings_options::{SETTINGS_FILE, SETTINGS_OPTIONS, SettingsOptions};

    let index = SETTINGS_OPTIONS
        .iter()
        .position(|option| option.id == SettingId::FilterProfanity)
        .unwrap();
    for filter_profanity in [false, true] {
        let layout =
            launcher::test_support::scratch(&format!("login-profanity-{filter_profanity}"));
        let mut saved = SettingsOptions::default();
        saved.set(index, i32::from(filter_profanity));
        saved
            .save(&layout.user_config_root.join(SETTINGS_FILE))
            .unwrap();
        let menu = MenuRuntime::new_with_layout(
            false,
            None,
            "Direct".to_owned(),
            layout,
            client_shell::player_skin::LocalPlayerSkin::generated_default("Direct"),
        );
        let settings = login_settings(&menu, None);
        assert_eq!(settings.filter_profanity, filter_profanity);
    }
}

// A hidden-menu direct launch must still report the player's saved GUI scale.
#[test]
fn login_settings_carry_the_saved_gui_scale_without_a_visible_menu() {
    let layout = launcher::test_support::scratch("login-settings");
    std::fs::create_dir_all(&layout.user_config_root).unwrap();
    std::fs::write(
        layout.user_config_root.join("video-settings.json"),
        r#"{"gui_scale_offset":-1}"#,
    )
    .unwrap();
    let menu = MenuRuntime::new_with_layout(
        false,
        None,
        "Direct".to_owned(),
        layout,
        client_shell::player_skin::LocalPlayerSkin::generated_default("Direct"),
    );
    let settings = login_settings(&menu, None);
    assert_eq!(settings.gui_scale_offset, -1);
    assert_eq!(settings.input_mode, protocol::PlayerInputMode::Mouse);
}

#[test]
fn login_settings_carry_the_saved_view_radius_without_a_visible_menu() {
    use launcher::menu::settings_options::{SETTINGS_FILE, SETTINGS_OPTIONS, SettingsOptions};

    let index = SETTINGS_OPTIONS
        .iter()
        .position(|option| option.id == SettingId::RenderDistance)
        .unwrap();
    for radius in [8, 35] {
        let layout = launcher::test_support::scratch(&format!("login-radius-{radius}"));
        let mut saved = SettingsOptions::default();
        saved.set(index, radius);
        saved
            .save(&layout.user_config_root.join(SETTINGS_FILE))
            .unwrap();
        let menu = MenuRuntime::new_with_layout(
            false,
            None,
            "Direct".to_owned(),
            layout,
            client_shell::player_skin::LocalPlayerSkin::generated_default("Direct"),
        );
        assert_eq!(
            login_settings(&menu, None).view_distance_chunks,
            radius as u8
        );
    }
}

#[test]
fn the_automatic_transfer_chain_is_bounded() {
    let mut controller = SessionController::default();

    // A user-initiated join always starts a fresh bounded chain.
    controller.begin_fresh_transfer_chain();
    for _ in 0..MAX_TRANSFER_CHAIN_HOPS {
        assert!(controller.consume_transfer_chain_hop());
    }
    assert!(
        !controller.consume_transfer_chain_hop(),
        "an exhausted chain must refuse to follow again"
    );

    // And another user join renews it after exhaustion.
    controller.begin_fresh_transfer_chain();
    assert!(controller.consume_transfer_chain_hop());
}

#[test]
fn identity_transfer_retains_local_proxy_login_address() {
    let mut controller = SessionController::default();
    let proxy = "127.0.0.1:19132";
    let identity = format!(
        "{}{}/{}",
        bridge::contract::NETHERNET_TARGET_PREFIX,
        bridge::contract::NETHERNET_SIGNALING_JSONRPC,
        "123456789"
    );
    assert_eq!(controller.select_login_address(proxy), proxy);
    assert_eq!(controller.select_login_address(&identity), proxy);
    assert_eq!(
        controller.select_login_address("example.com:19132"),
        "example.com:19132"
    );
    assert_eq!(controller.select_login_address(&identity), identity);
    controller.select_login_address(proxy);
    controller.begin_fresh_transfer_chain();
    assert_eq!(controller.select_login_address(&identity), identity);
}
