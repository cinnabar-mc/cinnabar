use super::*;
use bevy::prelude::*;
use render_scene::{ActorArtworkResource, WeatherTextureAssets};
use std::io::Cursor;

#[derive(Resource)]
struct ExpectedVisuals {
    sky_pixels: Vec<u8>,
    artwork: render_model::ActorArtworkPages,
    observations: usize,
}

/// Checks the resources consumed by world, UI and render publication in the current frame.
fn observe_visuals(
    weather: Res<WeatherTextureAssets>,
    artwork: Res<ActorArtworkResource>,
    reload: Res<PackReload>,
    mut expected: ResMut<ExpectedVisuals>,
) {
    assert_eq!(reload.completed_revision(), 1);
    assert_ne!(
        weather.identity(),
        WeatherTextureAssets::default().identity()
    );
    assert_eq!(
        weather.end_sky().unwrap().rgba8.as_ref(),
        expected.sky_pixels
    );
    assert_eq!(artwork.identity(), expected.artwork.identity());
    assert_eq!(artwork.pages(), expected.artwork.pages());
    expected.observations += 1;
}

#[test]
fn shared_frame_publishes_server_visuals_before_consumers() {
    let _sounds = client_audio::audio::SERVER_SOUNDS_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut png = Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(1, 1, image::Rgba([37, 59, 83, 255]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let sky_path = format!("{}.png", assets::weather_textures::END_SKY_TEXTURE_PATH);
    let stack = crate::runtime::network::pack_reload_tests::stack(&[(&sky_path, png.get_ref())]);
    let view = resource_pack::LayeredPackView::new(stack.clone());
    let weather = WeatherTextureAssets::default();
    let environment = prepare_environment(
        &view,
        &EnvironmentBase::new(Default::default(), None, weather.clone()),
        true,
        false,
    );
    let mut artwork = render_model::ActorArtworkPages::default();
    artwork.identity = [7; 32];
    artwork.pages = Arc::from([render_model::ActorTexturePage {
        width: 1,
        height: 1,
        layers: 1,
        rgba8: [11, 23, 47, 255].into(),
        multitexture: false,
    }]);
    // A texture-only replacement has artwork without new entity definitions.
    let application = PackApplication {
        admission: PackAdmission::Validated(stack),
        entity_artwork: Some(Arc::new(artwork.clone())),
        ..Default::default()
    };
    let assets = Arc::new(assets::RuntimeAssets::diagnostic());
    let mut app = App::new();
    crate::game_frame::configure_client_frame_schedule(&mut app);
    crate::game_frame::configure_pack_publication(&mut app);
    app.insert_resource(ClientWorld::new(assets.clone()))
        .insert_resource(crate::runtime::network::session_registries::tests::collision_resource())
        .insert_resource(UiRuntimeResource(client_ui::ui_runtime::UiRuntime::new(0)))
        .insert_resource(weather)
        .init_resource::<ActorArtworkResource>()
        .insert_resource(ExpectedVisuals {
            sky_pixels: vec![37, 59, 83, 255],
            artwork,
            observations: 0,
        })
        .add_systems(
            Update,
            (
                observe_visuals.in_set(client_state::ClientFrameSet::WorldPublication),
                observe_visuals.in_set(client_state::ClientFrameSet::UiPublication),
                observe_visuals.in_set(render_scene::ChunkRenderApplySet),
            ),
        );
    let mut reload = app.world_mut().resource_mut::<PackReload>();
    reload.begin_session(1, &application);
    // Deliver a completed worker result so this test exercises only frame publication.
    reload.ready = Some(Prepared {
        revision: reload.revision,
        generation: reload.generation,
        application,
        assets,
        environment: Some(environment),
        elapsed: Duration::ZERO,
    });
    app.update();
    assert_eq!(app.world().resource::<ExpectedVisuals>().observations, 3);
}

/// Prepares an original static atlas without installed game assets.
fn block_entity_artwork(height: u32) -> Arc<render_model::PreparedBlockEntityArtwork> {
    let width = render_model::MIN_DYNAMIC_ATLAS_WIDTH;
    let bytes = assets::encode_block_entity_catalog(
        b"{}",
        width,
        height,
        &vec![255; (width * height * 4) as usize],
        &[],
    )
    .unwrap();
    let carrier = assets::RuntimeBlockEntityAssets::decode(&bytes).unwrap();
    Arc::new(
        render_model::BlockEntityArtworkPreparer::default()
            .prepare(&carrier)
            .unwrap(),
    )
}

#[test]
fn rejected_block_entity_artwork_keeps_the_previous_pack_and_allows_retry() {
    let old_artwork = block_entity_artwork(8);
    let next_artwork = block_entity_artwork(4096);
    let mut scene = render_model::BlockEntityScene::default();
    scene.install_prepared_assets(&old_artwork);
    let mut resolved = 0;
    for _ in 0..256 {
        resolved = (0..256)
            .filter(|&key| {
                scene
                    .map_rect(key, || {
                        let [width, height] = render_model::MAP_CELL;
                        vec![7; (width * height * 4) as usize]
                    })
                    .is_some()
            })
            .count();
        scene.update(Default::default(), &[], &[]);
        if resolved == 256 {
            break;
        }
    }
    assert_eq!(resolved, 256);
    let old_atlas = scene.atlas().unwrap().clone();
    let old_assets = Arc::new(assets::RuntimeAssets::diagnostic());
    let next_assets = Arc::new(assets::RuntimeAssets::diagnostic());
    let old = PackApplication {
        block_entity_art: Some(old_artwork.clone()),
        server_lang: assets::ServerLangOverlay::from_layers(["item.stone.name=Old item"]),
        ..Default::default()
    };
    let next = PackApplication {
        block_entity_art: Some(next_artwork.clone()),
        server_lang: assets::ServerLangOverlay::from_layers(["item.stone.name=New item"]),
        terrain_identity: [7; 32],
        ..old.clone()
    };
    let mut ui = client_ui::ui_runtime::UiRuntime::new(0);
    ui.set_server_lang(old.server_lang.clone());
    let reload = PackReload {
        revision: 1,
        previous: Some(old),
        previous_assets: Some(old_assets.clone()),
        loading_screen_requested: true,
        ready: Some(Prepared {
            revision: 1,
            generation: 0,
            application: next.clone(),
            assets: next_assets.clone(),
            environment: None,
            elapsed: Duration::ZERO,
        }),
        ..Default::default()
    };
    let mut app = App::new();
    app.insert_resource(ClientWorld::new(old_assets.clone()))
        .insert_resource(UiRuntimeResource(ui))
        .insert_resource(render_scene::BlockEntitySceneResource(scene))
        .insert_resource(reload)
        .add_systems(Update, reload_resource_packs);
    app.update();
    let reload = app.world().resource::<PackReload>();
    assert_eq!(reload.completed_revision(), 1);
    assert!(reload.error().is_some());
    assert!(reload.progress().is_none());
    assert!(!reload.loading_screen());
    assert!(Arc::ptr_eq(
        reload
            .previous
            .as_ref()
            .unwrap()
            .block_entity_art
            .as_ref()
            .unwrap(),
        &old_artwork
    ));
    assert!(Arc::ptr_eq(
        reload.previous_assets.as_ref().unwrap(),
        &old_assets
    ));
    assert!(Arc::ptr_eq(
        app.world()
            .resource::<render_scene::BlockEntitySceneResource>()
            .atlas()
            .unwrap(),
        &old_atlas
    ));
    assert_eq!(
        app.world()
            .resource::<ClientWorld>()
            .terrain_resource_identity,
        [0; 32]
    );
    assert_eq!(
        app.world()
            .resource::<UiRuntimeResource>()
            .localized_item_name("minecraft:stone"),
        "Old item"
    );

    app.world_mut()
        .resource_mut::<render_scene::BlockEntitySceneResource>()
        .install_prepared_assets(&old_artwork);
    {
        let mut reload = app.world_mut().resource_mut::<PackReload>();
        reload.revision += 1;
        reload.ready = Some(Prepared {
            revision: reload.revision,
            generation: reload.generation,
            application: next,
            assets: next_assets.clone(),
            environment: None,
            elapsed: Duration::ZERO,
        });
    }
    app.update();
    let reload = app.world().resource::<PackReload>();
    assert_eq!(reload.completed_revision(), 2);
    assert!(reload.error().is_none());
    assert!(Arc::ptr_eq(
        reload
            .previous
            .as_ref()
            .unwrap()
            .block_entity_art
            .as_ref()
            .unwrap(),
        &next_artwork
    ));
    assert!(Arc::ptr_eq(
        reload.previous_assets.as_ref().unwrap(),
        &next_assets
    ));
    assert_eq!(
        app.world()
            .resource::<render_scene::BlockEntitySceneResource>()
            .atlas()
            .unwrap()
            .identity(),
        next_artwork.atlas().identity()
    );
    assert_eq!(
        app.world()
            .resource::<ClientWorld>()
            .terrain_resource_identity,
        [7; 32]
    );
    assert_eq!(
        app.world()
            .resource::<UiRuntimeResource>()
            .localized_item_name("minecraft:stone"),
        "New item"
    );
}
