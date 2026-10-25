use super::*;
use bevy::prelude::{Last, ResMut, Update};

#[derive(Resource)]
struct Submission {
    captured: client_session::CapturedPackets,
    frames: Vec<Vec<Vec<u8>>>,
}

/// Queues an early frame action through the production session FIFO.
fn queue_action(network: Res<NetworkHandle<()>>) {
    network
        .send_inventory_packet(protocol::modal_form_cancel_response(1))
        .unwrap();
}

/// Adds a late action that must remain in the same frame batch.
fn queue_late_action(network: Res<NetworkHandle<()>>) {
    network
        .send_inventory_packet(protocol::modal_form_cancel_response(2))
        .unwrap();
}

/// Observes submitted bytes at the point where the next frame may start waiting.
fn observe_before_wait(mut submission: ResMut<Submission>) {
    let frame = submission
        .captured
        .take_flushed_frame()
        .expect("the current frame must be submitted before its pacing wait");
    submission.frames.push(
        frame
            .iter()
            .map(|packet| protocol::encode(packet).unwrap().to_vec())
            .collect(),
    );
    assert!(submission.captured.take_flushed_frame().is_none());
}

#[test]
fn current_frame_is_submitted_before_pacing_in_either_install_order() {
    for pacing_first in [false, true] {
        let mut app = App::new();
        if pacing_first {
            render::FrameEnd::install(&mut app);
        }
        configure_network_frame_flush::<()>(&mut app);
        render::FrameEnd::install(&mut app);
        let (network, captured) = NetworkHandle::<()>::stub_capturing_packets();
        app.insert_resource(network)
            .insert_resource(Submission {
                captured,
                frames: Vec::new(),
            })
            .add_systems(Update, queue_action)
            .add_systems(Last, queue_late_action)
            .add_systems(render::FrameEnd, observe_before_wait);
        app.update();
        let expected: Vec<_> = [1, 2]
            .into_iter()
            .map(|id| {
                protocol::encode(&protocol::modal_form_cancel_response(id))
                    .unwrap()
                    .to_vec()
            })
            .collect();
        assert_eq!(app.world().resource::<Submission>().frames, [expected]);
    }
}

/// A frame counts as presented only when presenting took a swap-chain texture held before
/// rendering; no surface, or a present the renderer skipped, is not a presented frame.
#[test]
fn presentation_needs_a_released_swap_chain() {
    assert!(super::presented(1, 0));
    assert!(!super::presented(0, 0), "no window held a surface");
    assert!(!super::presented(1, 1), "the renderer skipped presenting");
}

#[test]
fn joined_idle_frames_advance_without_a_player_controller_or_world_stream() {
    let mut app = App::new();
    configure_network_frame_flush::<()>(&mut app);
    let (mut network, mut captured) = NetworkHandle::<()>::stub_capturing_packets();
    network.core_socket_dir = Some(PathBuf::from("unused-test-endpoint"));
    app.insert_resource(network)
        .init_resource::<bevy::prelude::Time<bevy::time::Real>>();
    for _ in 0..3 {
        app.world_mut()
            .resource_mut::<bevy::prelude::Time<bevy::time::Real>>()
            .advance_by(simulation_clock::TICK_DURATION / 4);
        app.update();
        assert!(captured.take_flushed_frame().is_none());
    }
    app.world_mut()
        .resource_mut::<bevy::prelude::Time<bevy::time::Real>>()
        .advance_by(simulation_clock::TICK_DURATION / 4);
    app.update();
    assert_eq!(captured.take_flushed_frame().unwrap().len(), 0);
    assert!(captured.take_flushed_frame().is_none());
}

/// Buffered pack destruction belongs to retirement, never to the frame exposing the menu.
#[test]
fn retirement_closes_old_ingress_and_releases_payloads_off_the_transition_thread() {
    use client_session::NetworkControlEvent;
    struct Pack(std::sync::mpsc::Sender<std::thread::ThreadId>);
    impl Drop for Pack {
        /// Identifies the thread that destroys the buffered session payload.
        fn drop(&mut self) {
            self.0.send(std::thread::current().id()).unwrap();
        }
    }
    let (dropped, released) = std::sync::mpsc::channel();
    let (mut old, ingress) = NetworkHandle::<Pack>::stub_with_control_sender();
    ingress
        .try_send(NetworkControlEvent::Bootstrap {
            session_generation: simulation_clock::identity::SessionGeneration::new(1),
            identity: Default::default(),
            world: protocol::WorldBootstrap {
                local_player_unique_id: 1,
                local_player_runtime_id: 1,
                dimension: 0,
                player_position: [0.0; 3],
                world_spawn_position: [0; 3],
                air_network_id: 0,
                block_network_ids_are_hashes: false,
            },
            environment: protocol::WorldEnvironmentBootstrap {
                initial_time: 0,
                day_cycle_lock_time: 0,
                daylight_cycle_enabled: true,
                weather_cycle_enabled: true,
                rain_level: 0.0,
                lightning_level: 0.0,
            },
            custom_blocks: Default::default(),
            inventory: protocol::InventoryEvent::Authority(protocol::InventoryAuthority::Server),
            item_registry: None,
            player_game_mode: protocol::PlayerGameMode::Survival,
            world_default_game_mode: protocol::GameModeUpdate::Explicit(
                protocol::PlayerGameMode::Survival,
            ),
            player_game_mode_uses_world_default: false,
            server_authoritative_block_breaking: false,
            rewind_history_size: 0,
            base_game_version: protocol::BaseGameVersion::Any,
            hardcore: false,
            hud_rules: Default::default(),
            death_rules: Default::default(),
            crafting_rules: Default::default(),
            packs: Pack(dropped),
            terrain_before_spawn: false,
            level_identity: None,
        })
        .unwrap_or_else(|_| panic!("fixture queue has room"));
    old.retire();
    assert!(
        ingress.is_closed(),
        "old ingress closes on the transition frame"
    );
    assert!(old.core_socket_dir().is_none());
    let thread = released
        .recv_timeout(std::time::Duration::from_secs(10))
        .unwrap();
    assert_ne!(
        thread,
        std::thread::current().id(),
        "buffered packs must be destroyed off the menu-transition thread"
    );
}

/// A replacement cannot observe the old session's already-buffered or late terminal controls.
#[test]
fn leave_then_immediate_rejoin_keeps_old_and_new_ingress_separate() {
    use client_session::NetworkControlEvent;
    let (mut network, old_ingress) = NetworkHandle::<()>::stub_with_control_sender();
    old_ingress
        .try_send(NetworkControlEvent::Transferred {
            target: client_session::SessionTransferTarget {
                host: "old.example".into(),
                port: 19132,
            },
            decode_error_count: 0,
        })
        .unwrap();
    network.retire();
    assert!(old_ingress.is_closed());
    let (replacement, new_ingress) = NetworkHandle::<()>::stub_with_control_sender();
    network = replacement;
    assert!(
        old_ingress
            .try_send(NetworkControlEvent::Stopped {
                decode_error_count: 1
            })
            .is_err()
    );
    new_ingress
        .try_send(NetworkControlEvent::Stopped {
            decode_error_count: 2,
        })
        .unwrap();
    assert!(matches!(
        network.control_events_mut().try_recv(),
        Ok(NetworkControlEvent::Stopped {
            decode_error_count: 2
        })
    ));
    assert!(network.control_events_mut().try_recv().is_err());
}
