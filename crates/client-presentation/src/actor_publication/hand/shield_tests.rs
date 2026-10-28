//! Decorated hand artwork keeps valid layers and survives early readiness reuse.
use super::*;
use bevy::ecs::system::SystemState;
use bevy::prelude::{PerspectiveProjection, World};

#[test]
fn decorated_shield_nonzero_source_layer_reaches_final_hand_submissions_as_layer_zero() {
    for java_mode in [false, true] {
        let (stream, mut equipment, artwork, mut input) =
            native_tests::fixture(Some(assets::gui_item::SHIELD_IDENTIFIER));
        input.main.as_mut().unwrap().shield_pattern = Some(Arc::new(render_api::ShieldPattern {
            base: 15,
            layers: Arc::from([]),
        }));
        let mut source = native_tests::hand_source_for_mode(
            &stream,
            &mut equipment,
            &artwork,
            &input,
            &mut java::HandCache::default(),
            0.5,
            java_mode,
        );
        let (artwork, locations) = artwork.with_equipment_rasters(&[
            render_model::EquipmentRaster {
                width: 2,
                height: 1,
                rgba8: [40, 50, 60, 255].repeat(2).into(),
            },
            render_model::EquipmentRaster {
                width: 2,
                height: 1,
                rgba8: [10, 20, 30, 255].repeat(2).into(),
            },
        ]);
        let location = locations[1].unwrap();
        assert!(location.layer() > 0);
        let (mut layer, _) = source.items[0].take().unwrap();
        layer.presentation.location = location;
        let atlas = decorated_item_atlas(
            &mut equipment,
            input.main.as_ref().unwrap(),
            &mut layer,
            &artwork,
        )
        .unwrap();
        assert_eq!(atlas.layers, 1);
        assert_eq!(&atlas.rgba8[..3], assets::banner::color_rgb(15));
        source.items[0] = Some((layer, atlas));
        for index in 0..2 {
            if index == 1 {
                source.items.swap(0, 1);
            }
            let submission = source.submissions()[index + 1].take().unwrap();
            let atlas = &source.items[index].as_ref().unwrap().1;
            assert_eq!(
                submission.texture_layer & render_model::hand::HAND_TEXTURE_LAYER_MASK,
                0
            );
            assert!(
                (submission.texture_layer & render_model::hand::HAND_TEXTURE_LAYER_MASK)
                    < atlas.layers
            );
            assert_ne!(
                submission.texture_layer & render_model::HAND_ITEM_LAYER_FLAG,
                0
            );
            assert_eq!(
                submission.texture_layer & render_model::HAND_OFFHAND_LAYER_FLAG != 0,
                index == 1
            );
        }
    }
}

#[test]
fn first_person_shield_readiness_reuses_composition_across_published_frames() {
    let ((mut stream, equipment, artwork, mut input), entities) =
        native_tests::fixture_with_clock(Some(assets::gui_item::SHIELD_IDENTIFIER), true);
    input.main.as_mut().unwrap().shield_pattern = Some(Arc::new(render_api::ShieldPattern {
        base: 15,
        layers: Arc::from([]),
    }));
    let mut world = World::new();
    let mut time = Time::<Real>::default();
    time.advance_by(simulation_clock::TICK_DURATION);
    world.insert_resource(time);
    world.insert_resource(equipment);
    world.insert_resource(render_scene::ActorArtworkResource(artwork));
    world.insert_resource(
        render_scene::ActorRenderScene::with_runtime_entity_assets(&entities).unwrap(),
    );
    world.insert_resource(super::super::HandRigBuilder::from_runtime_assets(&entities).unwrap());
    world.init_resource::<super::super::ActorFrameState>();
    world.init_resource::<super::super::PreparedActorPublication>();
    world.init_resource::<super::super::ActorFramePartialTick>();
    world.init_resource::<render_scene::HandRigScene>();
    let mut avatar = crate::local_player::LocalAvatarPresentation::default();
    avatar.begin_session(
        simulation_clock::identity::SessionGeneration::new(stream.authority().actor_session_id()),
        1,
    );
    world.insert_resource(avatar);
    world.init_resource::<crate::local_player::LocalAvatarVisibilityCarrier>();
    let mut settings = crate::camera::CameraSettingsAuthority::default();
    let mut user = ui::UserSettings::default();
    user.gameplay.default_perspective = semantic_input::PerspectiveMode::FirstPerson;
    user.video.java_animations = false;
    settings.replace(1, &user).unwrap();
    world.insert_resource(settings);
    world.insert_resource(crate::local_player::LocalViewPose::default());
    world.spawn((
        Transform::default(),
        Projection::Perspective(PerspectiveProjection::default()),
        crate::camera::FlyCamera::default(),
    ));
    let mut params = SystemState::<super::super::ActorFramePublication>::new(&mut world);
    let mut prepared_artwork = None;
    let mut retained = None;
    for _ in 0..3 {
        super::super::advance_actor_frame(
            super::super::ActorWorld {
                stream: Some(&mut stream),
                collisions: None,
                entity_assets: Some(&entities),
                pack_entities: None,
                session_items: None,
                prepared_actor_artwork: &mut prepared_artwork,
            },
            super::super::ActorFrameInput {
                cosmetics: Vec::new(),
                local_feed: None,
                predicted_eye: Some([0.0, 65.62, 0.0]),
                predicted_feet: Some([0.0, 64.0, 0.0]),
                local_equipment: input.clone(),
                swing_progress: None,
                renders_game: true,
                hide_hand: false,
                remote_emotes: Vec::new(),
                remote_hats: Vec::new(),
                custom_emote: None,
            },
            |_| {},
            |_, _| {
                (
                    None,
                    Some(actor_animation::AttachableAnimationInput {
                        first_person: true,
                        ..Default::default()
                    }),
                )
            },
            params.get_mut(&mut world).unwrap(),
        );
        let state = world.resource::<super::super::ActorFrameState>();
        assert!(state.hand_is_active());
        let pixels = Arc::clone(
            &state.hand_source.as_ref().unwrap().items[0]
                .as_ref()
                .unwrap()
                .1
                .rgba8,
        );
        assert_eq!(&pixels[..3], assets::banner::color_rgb(15));
        if let Some(previous) = retained.as_ref() {
            assert!(
                Arc::ptr_eq(previous, &pixels),
                "a visible first-person shield must retain its composition after readiness reuse"
            );
        }
        super::super::prepare_actor_render_frame(
            super::super::ActorWorld {
                stream: Some(&mut stream),
                collisions: None,
                entity_assets: Some(&entities),
                pack_entities: None,
                session_items: None,
                prepared_actor_artwork: &mut prepared_artwork,
            },
            None,
            |_, _, _, _| false,
            None,
            params.get_mut(&mut world).unwrap(),
        );
        let frame = world
            .resource::<render_scene::HandRigScene>()
            .frame
            .as_ref()
            .unwrap();
        assert!(Arc::ptr_eq(
            &pixels,
            &frame.item_atlases[0].as_ref().unwrap().rgba8
        ));
        retained = Some(pixels);
    }
}
