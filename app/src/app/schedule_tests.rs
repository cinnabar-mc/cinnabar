use crate::runtime::network::session::NetworkConfig;
use bevy::ecs::schedule::{ScheduleLabel, Schedules};
use simulation_clock::identity::SessionGeneration;

use super::*;

#[test]
fn network_config_call_sites_share_the_process_blob_cache() {
    let owner = ClientBlobCacheOwner::default();
    let registry_base = crate::runtime::network::session_registries::tests::registry_base();
    let first = NetworkConfig {
        registry_base: registry_base.clone(),
        session_generation: SessionGeneration::new(7),
        socket_dir: std::path::PathBuf::from("first-core.sock"),
        display_name: "cache-owner".to_owned(),
        client_random_id: protocol::ClientRandomId::new(-7).unwrap(),
        client_blob_cache: owner.cache(),
        player_skin: client_shell::player_skin::LocalPlayerSkin::generated_default("cache-owner"),
        login_settings: protocol::LoginSettings::default(),
        actor_artwork: None,
        ui_catalog: None,
        log_dir: std::path::PathBuf::from("logs"),
        record_session: false,
    };
    let hash = first
        .client_blob_cache
        .as_ref()
        .unwrap()
        .insert(b"verified-across-session")
        .expect("seed verified blob before replacement");
    let replacement = NetworkConfig {
        registry_base,
        session_generation: SessionGeneration::new(8),
        socket_dir: std::path::PathBuf::from("replacement-core.sock"),
        display_name: "cache-owner".to_owned(),
        client_random_id: protocol::ClientRandomId::new(-7).unwrap(),
        client_blob_cache: owner.cache(),
        player_skin: client_shell::player_skin::LocalPlayerSkin::generated_default("cache-owner"),
        login_settings: protocol::LoginSettings::default(),
        actor_artwork: None,
        ui_catalog: None,
        log_dir: std::path::PathBuf::from("logs"),
        record_session: false,
    };

    assert!(replacement.client_blob_cache.unwrap().contains(hash));
}

#[test]
fn production_update_schedule_initializes_without_dependency_cycles() {
    let mut app = App::new();
    configure_client_frame_schedule(&mut app);
    app.add_plugins(FlyCameraPlugin::default());
    configure_client_production_frame_systems(&mut app);
    configure_client_runtime_frame_systems(&mut app);
    session::configure_finish_system(&mut app);

    let mut schedules = app
        .world_mut()
        .remove_resource::<Schedules>()
        .expect("Schedules resource");
    let result = schedules
        .get_mut(Update)
        .expect("production Update schedule")
        .initialize(app.world_mut());
    app.world_mut().insert_resource(schedules);

    assert!(
        result.is_ok(),
        "production Update schedule: {:?}",
        result.err()
    );
}

/// Systems, including auto-inserted sync points, and run conditions the client's own frame
/// registrations dispatch every `Update` and `PostUpdate`. Each one costs executor and parameter
/// work on every frame even when it finds nothing to do, so growth must be deliberate: change
/// these with the registrations, and prefer one set condition over a condition per system.
const FRAME_DISPATCH_BUDGET: [(&str, usize, usize); 2] = [("Update", 143, 8), ("PostUpdate", 1, 0)];

#[test]
fn client_frame_dispatch_stays_within_its_budget() {
    let mut app = App::new();
    configure_client_frame_schedule(&mut app);
    app.add_plugins(FlyCameraPlugin::default());
    configure_client_production_frame_systems(&mut app);
    configure_client_runtime_frame_systems(&mut app);
    session::configure_finish_system(&mut app);
    let mut schedules = app
        .world_mut()
        .remove_resource::<Schedules>()
        .expect("Schedules resource");
    let mut dispatched = Vec::new();
    for (name, label) in [
        ("Update", Update.intern()),
        ("PostUpdate", bevy::prelude::PostUpdate.intern()),
    ] {
        let Some(schedule) = schedules.get_mut(label) else {
            dispatched.push((name, 0, 0));
            continue;
        };
        let graph = schedule.graph();
        let conditions = graph
            .systems
            .iter()
            .map(|(_, _, conditions)| conditions.len())
            .chain(
                graph
                    .system_sets
                    .iter()
                    .map(|(_, _, conditions)| conditions.len()),
            )
            .sum::<usize>();
        schedule
            .initialize(app.world_mut())
            .unwrap_or_else(|error| panic!("{name} builds: {error:?}"));
        dispatched.push((name, schedule.systems_len(), conditions));
    }
    app.world_mut().insert_resource(schedules);
    assert_eq!(
        dispatched, FRAME_DISPATCH_BUDGET,
        "(schedule, systems, run conditions) dispatched per frame"
    );
}

/// Every host shares the world wiring, so each one follows the vehicle with the rider's view.
#[test]
fn shared_world_wiring_installs_vehicle_look_following() {
    let mut app = App::new();
    crate::game_frame::configure_world(&mut app);
    assert!(
        app.world()
            .contains_resource::<client_presentation::camera::VehicleLookFollow>()
    );
}
