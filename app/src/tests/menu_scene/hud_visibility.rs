//! HUD visibility through both first-person hand publishers and retained name tags.

use crate::runtime::network::resource_packs::PackApplication;
use crate::ui_runtime::presentation::publish::PreparedUiPublication;
use bevy::window::{PrimaryWindow, Window};
use client_presentation::environment::{WeatherState, WorldClock};
use client_runtime::network::NetworkHandle;
use protocol::identity::FifoSequence;
use simulation_clock::PhysicsTick;
use simulation_clock::identity::SessionGeneration;
use {
    super::*, client_presentation::actor_publication::HandRigBuilder,
    client_ui::test_support::mini_engine_presentation,
};
use {
    crate::{
        runtime::{visibility::CaveVisibilityCache, world::WorldStreamFramePoll},
        ui_runtime::presentation::prepare_ui_runtime,
    },
    client_interaction::{item_use::ItemUseRuntime, movement::MovementTicker},
    client_presentation::{
        camera::{CameraSettingsAuthority, FlyCamera, perspective_pose},
        local_player::{LocalPlayerFrameCarrier, LocalPlayerFrameSample},
    },
};

use launcher::menu::settings_options::SettingId;
/// Builds a renderer-free world with gameplay overlays available.
pub(super) fn fixture_world() -> World {
    let (_pack, _geometry, entities) = super::super::viewmodel_presentation::hand_fixture();
    let scene =
        render_scene::ActorRenderScene::with_runtime_entity_assets_and_equipment(&entities, &[])
            .unwrap();
    let icons = Arc::new(
        assets::RuntimeIconCatalog::decode(
            &assets::encode_icon_catalog([0; 32], &[], &[]).unwrap(),
        )
        .unwrap(),
    );
    let (equipment, artwork, _) =
        client_presentation::presentation::equipment::EquipmentRuntime::build(
            entities.clone(),
            None,
            icons,
            None,
            None,
            render_model::ActorArtworkPages::default(),
        );
    let mut client = player_world(entities.clone());
    let stream = client.stream.as_mut().unwrap();
    stream
        .submit(
            3,
            WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                dimension: 0,
                unique_id: 2,
                runtime_id: 2,
                kind: ActorKind::Player {
                    uuid: [2; 16],
                    username: "A".into(),
                },
                position: [0., 64., 2.],
                velocity: [0.; 3],
                pitch: 0.,
                yaw: 0.,
                head_yaw: 0.,
                body_yaw: 0.,
                held_item: Default::default(),
                metadata: Arc::from([protocol::ActorMetadata {
                    key: protocol_ids::ActorDataKey::AlwaysShowNameTag.wire_value(),
                    value: protocol::ActorMetadataValue::Byte(1),
                }]),
                attributes: Arc::from([]),
                properties: Arc::from([]),
                links: Arc::from([]),
            })),
        )
        .unwrap();
    stream.advance_actor_interpolation_frame(2);
    let mut world = super::super::actor_frame_allocations::actor_frame_world(
        client,
        scene,
        artwork,
        HandRigBuilder::from_runtime_assets(&entities).unwrap(),
        (Vec3::new(0., 66., -2.), Vec3::new(0., 66., 0.)),
    );
    world.insert_resource(equipment);
    world.insert_resource(UiRuntimeResource(UiRuntime::new(1)));
    world.insert_resource(UiPresentationResource(mini_engine_presentation()));
    world.insert_resource(MenuRuntime::new(false, 2, "Tester".into()));
    world.insert_resource(NetworkHandle::<PackApplication>::stub().0);
    world.init_resource::<PreparedUiPublication>();
    world.init_resource::<CaveVisibilityCache>();
    world.init_resource::<render_scene::VisibilityDiagnosticsInput>();
    world.init_resource::<render_scene::VisibilityDiagnostics>();
    world.init_resource::<render_scene::ChunkRenderQueue>();
    world.init_resource::<render_scene::ChunkUploadAcknowledgements>();
    world.init_resource::<WorldStreamFramePoll>();
    world.init_resource::<LocalPlayerFrameCarrier>();
    world.init_resource::<WorldClock>();
    world.init_resource::<WeatherState>();
    world.init_resource::<ItemUseRuntime>();
    world.init_resource::<client_interaction::melee::MeleeRuntime>();
    world.init_resource::<MovementTicker>();
    world.spawn((Window::default(), PrimaryWindow));
    let camera = world
        .query_filtered::<Entity, With<FlyCamera>>()
        .single(&world)
        .unwrap();
    world
        .entity_mut(camera)
        .insert((Camera::default(), Camera3d::default()));
    world
}

/// Checks the published hand and label state after one visibility update.
fn assert_published(world: &mut World, hands: bool, names: bool) {
    // An inactive animated rig selects the real CPU-hand capture path.
    world.resource_mut::<render_scene::HandRigScene>().clear();
    world.run_system_cached(prepare_ui_runtime).unwrap();
    let pending = world
        .resource::<PreparedUiPublication>()
        .0
        .as_ref()
        .unwrap();
    assert_eq!(pending.preview.hands, hands, "CPU hand capture");
    assert_eq!(
        world
            .resource_mut::<UiPresentationResource>()
            .hud_frame_mut()
            .first_person,
        hands,
        "HUD hand ownership"
    );
    assert_eq!(
        !world
            .resource_mut::<UiPresentationResource>()
            .nametag_scene()
            .records
            .is_empty(),
        names,
        "retained nametag publication"
    );
    // Terrain startup is unrelated to the visibility option under test.
    world
        .resource_mut::<UiPresentationResource>()
        .set_loading_stage(None);
    world
        .run_system_cached(crate::runtime::network::advance_actor_frame)
        .unwrap();
    world.run_system_cached(prepare_actor_render_frame).unwrap();
    assert_eq!(
        world.resource::<render_scene::HandRigScene>().is_active(),
        hands,
        "animated hand publication"
    );
}

#[test]
fn hud_coordinates_floor_player_feet_independently_of_camera_pose() {
    let mut world = fixture_world();
    let camera = world
        .query_filtered::<Entity, With<FlyCamera>>()
        .single(&world)
        .unwrap();
    let feet = Vec3::new(-16.5, 47.0, 2.5);
    let eye = feet + Vec3::Y * protocol::PLAYER_NETWORK_OFFSET;
    let rotation = Quat::from_euler(EulerRot::YXZ, 0.8, -0.3, 0.0);
    for perspective in [
        semantic_input::PerspectiveMode::FirstPerson,
        semantic_input::PerspectiveMode::ThirdPersonBack,
        semantic_input::PerspectiveMode::ThirdPersonFront,
    ] {
        assert_eq!(
            world.resource::<CameraSettingsAuthority>().perspective(),
            perspective
        );
        let pose = perspective_pose(eye, rotation, perspective);
        world.entity_mut(camera).insert(pose);
        world
            .resource_mut::<LocalPlayerFrameCarrier>()
            .publish(LocalPlayerFrameSample {
                session_generation: SessionGeneration::new(1),
                actor_session_id: 1,
                fifo_sequence: FifoSequence::new(1),
                physics_tick: PhysicsTick::new(1),
                perspective,
                world_collision_identity: sim::CollisionQuery::synthetic(()).identity,
                pose,
                eye,
                feet,
                rotation,
            })
            .unwrap();
        world.run_system_cached(prepare_ui_runtime).unwrap();
        assert_eq!(
            world
                .resource::<UiPresentationResource>()
                .hud_frame()
                .player_block,
            Some([-17, 47, 2]),
            "HUD coordinates must follow floored feet in {perspective:?}"
        );
        world
            .resource_mut::<CameraSettingsAuthority>()
            .cycle_perspective();
    }
}

#[test]
fn hide_hud_clears_published_hands_and_nametags_and_restores_the_hand_preference() {
    let mut world = fixture_world();
    let mut prior_hand = None;
    for (hide_hud, hide_hand, hands, names) in [
        (false, false, true, true),
        (true, false, false, false),
        (false, false, true, true),
        (false, true, false, true),
        (true, true, false, false),
        (false, true, false, true),
        (false, false, true, true),
    ] {
        {
            let mut menu = world.resource_mut::<MenuRuntime>();
            if prior_hand != Some(hide_hand) {
                menu.set_session_option(SettingId::HideHand, Some(i32::from(hide_hand)));
                prior_hand = Some(hide_hand);
            }
            menu.set_session_option(SettingId::HideHud, Some(i32::from(hide_hud)));
        }
        assert_published(&mut world, hands, names);
        assert_eq!(
            world
                .resource::<MenuRuntime>()
                .settings_snapshot()
                .0
                .value(SettingId::HideHand),
            i32::from(hide_hand),
            "HUD visibility must preserve the independent hand preference"
        );
    }
}

/// Preparation and publication across frames, as the schedule runs them: an idle frame repeats
/// without building, and a HUD setting changed through the menu rebuilds the next frame.
#[test]
fn idle_frames_repeat_and_a_hud_setting_change_rebuilds() {
    use crate::ui_runtime::presentation::publish_ui_runtime;
    let mut world = fixture_world();
    world.init_resource::<render_scene::UiRenderSceneResource>();
    world.init_resource::<render_scene::UiRenderStatsResource>();
    world
        .resource_mut::<UiPresentationResource>()
        .startup_mut()
        .release(1);
    // A shown paper doll sways with the time and builds every frame.
    world
        .resource_mut::<MenuRuntime>()
        .set_session_option(SettingId::HidePaperdoll, Some(1));
    world
        .resource_mut::<UiPresentationResource>()
        .set_frame_reuse(client_ui::ui_runtime::presentation::FrameReuse::Skip);
    let frame = |world: &mut World| {
        let built = world.resource::<UiPresentationResource>().frames_built();
        world.run_system_cached(prepare_ui_runtime).unwrap();
        world.run_system_cached(publish_ui_runtime).unwrap();
        world.increment_change_tick();
        world.resource::<UiPresentationResource>().frames_built() != built
    };
    assert!(frame(&mut world), "the first frame builds");
    test_time::eventually("an idle frame repeats", || !frame(&mut world));
    assert!(!frame(&mut world), "idle frames keep repeating");
    world
        .resource_mut::<MenuRuntime>()
        .set_session_option(SettingId::HideHud, Some(1));
    assert!(frame(&mut world), "hiding the HUD rebuilds the frame");
    test_time::eventually("the hidden HUD repeats", || !frame(&mut world));
}

/// Publication marks neither the UI runtime nor the presentation changed, whether it builds or
/// repeats the frame, so their change ticks revision only what other systems wrote.
#[test]
fn publication_leaves_the_ui_runtime_and_presentation_unchanged() {
    use crate::ui_runtime::presentation::publish_ui_runtime;
    use bevy::prelude::DetectChanges;
    let mut world = fixture_world();
    world.init_resource::<render_scene::UiRenderSceneResource>();
    world.init_resource::<render_scene::UiRenderStatsResource>();
    world
        .resource_mut::<UiPresentationResource>()
        .startup_mut()
        .release(1);
    for _ in 0..4 {
        world.run_system_cached(prepare_ui_runtime).unwrap();
        world.increment_change_tick();
        let ticks = |world: &World| {
            (
                world.resource_ref::<UiRuntimeResource>().last_changed(),
                world
                    .resource_ref::<UiPresentationResource>()
                    .last_changed(),
            )
        };
        let before = ticks(&world);
        world.run_system_cached(publish_ui_runtime).unwrap();
        assert_eq!(
            ticks(&world),
            before,
            "publication marked the UI runtime or presentation changed"
        );
        world.increment_change_tick();
    }
}
