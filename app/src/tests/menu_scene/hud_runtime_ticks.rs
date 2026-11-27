//! Runtime changes made while preparation returns early still reach the HUD.

use super::hud_visibility::fixture_world;
use crate::ui_runtime::presentation::prepare_ui_runtime;
use bevy::window::{PrimaryWindow, Window};
use {super::*, protocol::NetworkItemStack};

/// Puts `count` stone in the first hotbar slot, as the server's inventory content would.
fn hold(world: &mut World, count: u16) {
    world
        .resource_mut::<PlayerRuntime>()
        .inventory
        .ledger_mut()
        .apply(&protocol::InventoryEvent::Content(
            protocol::InventoryContentEvent {
                container: protocol::ContainerIdentity::window(
                    protocol::PLAYER_INVENTORY_WINDOW_ID,
                ),
                slots: vec![NetworkItemStack {
                    network_id: 1,
                    count,
                    stack_network_id: 1,
                    ..NetworkItemStack::empty()
                }]
                .into(),
                storage_item: Default::default(),
            },
        ));
}

/// Sizes the primary window; a zero size makes preparation return before capturing.
fn resize(world: &mut World, width: u32, height: u32) {
    let mut windows = world.query_filtered::<&mut Window, With<PrimaryWindow>>();
    windows
        .single_mut(world)
        .unwrap()
        .resolution
        .set_physical_resolution(width, height);
}

fn hotbar_count(world: &World) -> Option<u16> {
    world
        .resource::<UiPresentationResource>()
        .hud_frame()
        .hotbar_stacks[0]
        .as_ref()
        .map(|stack| stack.count)
}

/// A hotbar change made while the window has no size, so preparation returns early, shows once
/// the window can draw again.
#[test]
fn a_hotbar_change_during_a_zero_size_window_reaches_the_hud() {
    let mut world = fixture_world();
    world
        .resource_mut::<UiPresentationResource>()
        .set_loading_stage(None);
    hold(&mut world, 2);
    for _ in 0..3 {
        world.run_system_cached(prepare_ui_runtime).unwrap();
    }
    assert_eq!(hotbar_count(&world), Some(2));
    resize(&mut world, 0, 0);
    hold(&mut world, 5);
    world.run_system_cached(prepare_ui_runtime).unwrap();
    resize(&mut world, 1280, 720);
    world.run_system_cached(prepare_ui_runtime).unwrap();
    assert_eq!(
        hotbar_count(&world),
        Some(5),
        "the change made while hidden"
    );
}

/// Publication finishes preparing the closed inventory, which stops the capture naming its items,
/// without marking anything: the next capture runs, and later ones repeat, which debug builds
/// check against a fresh capture.
#[test]
fn a_held_item_keeps_reaching_the_hud_after_publication_prepares_the_inventory() {
    use crate::ui_runtime::presentation::publish_ui_runtime;
    let mut world = fixture_world();
    world.init_resource::<render_scene::UiRenderSceneResource>();
    world.init_resource::<render_scene::UiRenderStatsResource>();
    world
        .resource_mut::<UiPresentationResource>()
        .startup_mut()
        .release(1);
    hold(&mut world, 2);
    for _ in 0..6 {
        world.run_system_cached(prepare_ui_runtime).unwrap();
        world.run_system_cached(publish_ui_runtime).unwrap();
        world.increment_change_tick();
    }
    assert_eq!(hotbar_count(&world), Some(2));
}
