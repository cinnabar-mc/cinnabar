use super::*;
use crate::runtime::world::ClientWorld;
use bevy::ecs::{change_detection::DetectChanges, system::RunSystemOnce};
use render_scene::{WorldPointLight, WorldPointLightSource};
use std::sync::Arc;

/// Builds a world with one original emissive block for held and dropped item tests.
fn emissive_world() -> ClientWorld {
    let assets = Arc::new(
        assets::RuntimeAssets::diagnostic()
            .with_block_overlay(
                1,
                &assets::BlockOverlay {
                    visuals: vec![assets::BlockVisual::diagnostic(
                        assets::BlockFlags::empty(),
                        assets::ContributorRole::Primary,
                    )],
                    light_properties: vec![assets::LightProperties::new(15, 0).unwrap()],
                    materials: vec![assets::Material::unvaried()],
                    ..Default::default()
                },
            )
            .unwrap(),
    );
    let mut world = ClientWorld::new(assets.clone());
    world.stream = Some(chunk_pipeline::WorldStream::new_with_assets(
        protocol::WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            local_player_unique_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        assets,
        [0.0; 3],
        None,
    ));
    world
}

/// Returns a nonempty item whose block descriptor resolves to the emissive fixture.
fn emissive_stack() -> protocol::NetworkItemStack {
    protocol::NetworkItemStack {
        network_id: protocol::vanilla_item_registry()
            .iter()
            .find(|entry| entry.identifier.as_ref() == "minecraft:glowstone")
            .unwrap()
            .network_id,
        count: 1,
        block_runtime_id: 1,
        ..Default::default()
    }
}

/// A menu whose Dynamic Lights setting is `enabled`.
fn dynamic_lights_menu(enabled: bool) -> client_shell::menu::MenuRuntime {
    let mut menu = client_shell::menu::MenuRuntime::new(false, 2, "Tester".into());
    menu.set_session_option(
        launcher::menu::settings_options::SettingId::DynamicLights,
        Some(i32::from(enabled)),
    );
    menu
}

#[test]
fn held_light_requires_a_published_local_position_and_clears_on_reset() {
    let mut player = PlayerRuntime(player_state::PlayerState::new(1));
    player.inventory.set_local_selected_slot(0);
    player
        .inventory
        .ledger_mut()
        .apply(&protocol::InventoryEvent::Slot(
            protocol::InventorySlotEvent {
                identity: protocol::SlotIdentity {
                    container: protocol::ContainerIdentity::window(0),
                    slot: 0,
                },
                stack: emissive_stack(),
                storage_item: None,
            },
        ));
    assert!(player.selected_stack().is_some());
    let mut world = bevy::prelude::World::new();
    world.insert_resource(emissive_world());
    world.insert_resource(player);
    world.init_resource::<LocalPlayerFrameCarrier>();
    world.init_resource::<client_presentation::actor_publication::ActorFramePartialTick>();
    world.init_resource::<WorldPointLight>();

    world
        .run_system_once(crate::local_player::publish_world_point_lights)
        .unwrap();
    assert_eq!(
        world.resource::<WorldPointLight>().local,
        WorldPointLightSource::default()
    );

    let sample = frozen_local_player_sample();
    let position = (sample.feet + Vec3::Y).to_array();
    world
        .resource_mut::<LocalPlayerFrameCarrier>()
        .publish(sample)
        .unwrap();
    world
        .run_system_once(crate::local_player::publish_world_point_lights)
        .unwrap();
    let local = world.resource::<WorldPointLight>().local;
    assert_eq!(local.position, position);
    assert!(local.intensity > 0.0);
    assert!(local.radius > 0.0);

    world
        .resource_mut::<LocalPlayerFrameCarrier>()
        .reset(LocalPlayerFrameReset::Session);
    world
        .run_system_once(crate::local_player::publish_world_point_lights)
        .unwrap();
    assert_eq!(
        world.resource::<WorldPointLight>().local,
        WorldPointLightSource::default()
    );
}

#[test]
fn additional_lights_require_a_published_position_even_without_a_local_light() {
    let mut client = emissive_world();
    client
        .stream
        .as_mut()
        .unwrap()
        .submit(
            1,
            protocol::WorldEvent::Actor(protocol::ActorEvent::Spawn(protocol::ActorSpawnEvent {
                dimension: 0,
                unique_id: 2,
                runtime_id: 2,
                kind: protocol::ActorKind::Entity {
                    identifier: "minecraft:item".into(),
                },
                position: [1.0, 2.0, 3.0],
                velocity: [0.0; 3],
                pitch: 0.0,
                yaw: 0.0,
                head_yaw: 0.0,
                body_yaw: 0.0,
                held_item: emissive_stack(),
                metadata: Arc::from([]),
                attributes: Arc::from([]),
                properties: Arc::from([]),
                links: Arc::from([]),
            })),
        )
        .unwrap();
    let mut world = bevy::prelude::World::new();
    world.insert_resource(client);
    world.init_resource::<LocalPlayerFrameCarrier>();
    world.init_resource::<client_presentation::actor_publication::ActorFramePartialTick>();
    world.insert_resource(dynamic_lights_menu(true));
    world.init_resource::<WorldPointLight>();

    world
        .run_system_once(crate::local_player::publish_world_point_lights)
        .unwrap();
    assert!(
        world
            .resource::<WorldPointLight>()
            .additional
            .iter()
            .all(|light| *light == WorldPointLightSource::default())
    );

    world
        .resource_mut::<LocalPlayerFrameCarrier>()
        .publish(frozen_local_player_sample())
        .unwrap();
    world
        .run_system_once(crate::local_player::publish_world_point_lights)
        .unwrap();
    let lights = world.resource::<WorldPointLight>();
    assert!(lights.enabled);
    assert_eq!(lights.local.intensity, 0.0);
    assert!(lights.additional[0].intensity > 0.0);

    // Turning Dynamic Lights off clears the item lights and leaves the frame unchanged after.
    world.insert_resource(dynamic_lights_menu(false));
    world
        .run_system_once(crate::local_player::publish_world_point_lights)
        .unwrap();
    let lights = world.resource::<WorldPointLight>();
    assert!(!lights.enabled);
    assert!(
        lights
            .additional
            .iter()
            .all(|light| *light == WorldPointLightSource::default())
    );
    let unchanged = world.change_tick();
    world
        .run_system_once(crate::local_player::publish_world_point_lights)
        .unwrap();
    assert!(
        !world
            .resource_ref::<WorldPointLight>()
            .last_changed()
            .is_newer_than(unchanged, world.change_tick())
    );

    world.insert_resource(dynamic_lights_menu(true));
    world
        .run_system_once(crate::local_player::publish_world_point_lights)
        .unwrap();
    assert!(world.resource::<WorldPointLight>().additional[0].intensity > 0.0);
    world
        .resource_mut::<LocalPlayerFrameCarrier>()
        .reset(LocalPlayerFrameReset::Dimension);
    world
        .run_system_once(crate::local_player::publish_world_point_lights)
        .unwrap();
    assert!(
        world
            .resource::<WorldPointLight>()
            .additional
            .iter()
            .all(|light| *light == WorldPointLightSource::default())
    );
}
