use super::*;
use protocol::InventoryOptions;
use std::sync::Arc;

use launcher::menu::settings_options::SettingsOptions;

fn plain_inventory(on: bool) -> Arc<SettingsOptions> {
    let mut settings = SettingsOptions::default();
    settings.set(SettingId::OpenPlainInventory.index(), i32::from(on));
    Arc::new(settings)
}

/// A player and runtime whose stored layout is `stored`, in creative or survival.
fn session(creative: bool, stored: InventoryLayout) -> (player_state::PlayerState, UiRuntime) {
    let mut player = player_state::PlayerState::new(1);
    let runtime = crate::test_support::inventory_session(&mut player);
    if creative {
        player
            .facts
            .publish_player_game_mode(PlayerGameMode::Creative);
    }
    player.inventory.set_inventory_options(InventoryOptions {
        inventory_layout: stored,
        ..Default::default()
    });
    (player, runtime)
}

#[test]
fn the_plain_inventory_setting_overrides_the_stored_layout_in_creative_and_survival() {
    for (creative, stored) in [
        (true, InventoryLayout::RecipeBookOnly),
        (false, InventoryLayout::RecipeBook),
    ] {
        let (player, mut runtime) = session(creative, stored);
        // Off, which is the default: the stored layout stands.
        assert_eq!(
            runtime.inventory_controller(&player).unwrap().layout(),
            stored,
            "creative: {creative}"
        );
        runtime.set_control_settings(plain_inventory(true));
        assert_eq!(
            runtime.inventory_controller(&player).unwrap().layout(),
            InventoryLayout::InventoryOnly,
            "creative: {creative}"
        );
        // Only the screen's starting layout: the stored tuple, which is what the server is
        // told, is left alone.
        assert_eq!(
            player.inventory.inventory_options().inventory_layout,
            stored
        );
        runtime.set_control_settings(plain_inventory(false));
        assert_eq!(
            runtime.inventory_controller(&player).unwrap().layout(),
            stored
        );
    }
}

#[test]
fn the_player_can_still_open_the_recipe_book_from_a_plain_inventory() {
    let (mut player, mut runtime) = session(true, InventoryLayout::RecipeBookOnly);
    runtime.set_control_settings(plain_inventory(true));
    runtime.toggle_inventory(&mut player, None);
    assert_eq!(
        runtime.inventory_controller(&player).unwrap().layout(),
        InventoryLayout::InventoryOnly
    );
    // The book button asks for the recipe book layout (index 2): this screen keeps it, the
    // player's choice is saved as usual, and the setting does not take it back mid-screen.
    runtime
        .select_inventory_layout(&mut player, 2)
        .expect("the recipe book layout is offered");
    assert_eq!(
        runtime.inventory_controller(&player).unwrap().layout(),
        InventoryLayout::RecipeBook
    );
    assert_eq!(
        player.inventory.inventory_options().inventory_layout,
        InventoryLayout::RecipeBook
    );
}
