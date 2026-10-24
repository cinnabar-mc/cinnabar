//! Prepared item models retain source identity and session artwork priority.
use super::*;

#[test]
fn rejected_session_artwork_falls_back_to_dedicated_geometry() {
    let identifier = "minecraft:chest";
    let (mut icons, stream) = fixture_item(identifier, 0, 0);
    checkerboard_session_icon(&mut icons, identifier);
    let mut world = World::new();
    world.init_resource::<DroppedItemScene>();
    chest_artwork(&mut world, Some(30));
    let mut publisher = SystemState::<DroppedItemPublisher<'_, '_>>::new(&mut world);
    publisher.get_mut(&mut world).unwrap().publish(
        Some(&icons),
        Some(&stream),
        None,
        (None, None),
        Default::default(),
        (None, Default::default()),
    );
    let scene = world.resource::<DroppedItemScene>();
    assert_eq!(scene.instances.len(), 1);
    let DroppedItemModel::Block(model) = &scene.models[scene.instances[0].model as usize] else {
        panic!("rejected session artwork must allow the dedicated startup model");
    };
    assert_eq!(model.materials[0].0.rgba8[0], 30);
}

#[test]
fn dedicated_plans_charge_complete_geometry_and_reuse_cached_costs() {
    let (icons, stream) = fixture_item("minecraft:chest", 0, 0);
    let mut world = World::new();
    chest_artwork(&mut world, Some(30));
    let source = world
        .resource::<render_scene::BlockEntitySceneResource>()
        .0
        .held_models()
        .unwrap();
    let (key, _, model) = special::model(Some(source), "minecraft:chest").unwrap();
    let model = DroppedItemModel::Block(model.clone());
    let cost = render_model::dropped_item::ItemModelCost::from_model(&model).unwrap();
    assert_eq!(cost.vertices, model.mesh(cost.side, 1).unwrap().len());
    let mut dropped = Vec::new();
    stream.authority().dropped_items_into(0.0, &mut dropped);
    let view = &dropped[0];
    let preparation = DropPreparation {
        view,
        native_origin: view.position,
        camera: None,
        frustum: None,
    };
    let mut cache = ModelCache::default();
    for vertices in [cost.vertices - 1, cost.vertices, cost.vertices] {
        cache.budget = render_model::dropped_item::ItemModelBudget::new(
            assets::texture::policy::device_policy(),
            render_model::MAX_ITEM_LAYERS,
            (vertices * render_model::dropped_item::ITEM_MESH_VERTEX_BYTES) as u64,
        );
        let prepared = cache
            .prepare(|cache| {
                DroppedItemPublisher::dropped_model(
                    cache,
                    &icons,
                    None,
                    Some(source),
                    None,
                    &preparation,
                )
            })
            .unwrap();
        let mut instances = Vec::new();
        prepared.append(view, &mut instances, |_| (0, 15));
        let slot = instances[0].model as usize;
        assert_eq!(
            matches!(cache.models[slot], DroppedItemModel::Block(_)),
            vertices == cost.vertices
        );
        cache.finish_frame();
        assert!(
            !matches!(cache.shared[slot], DroppedItemModel::Vacant),
            "planned artwork must survive upload admission"
        );
        assert_eq!(cache.mesh_validations[&key], 1);
    }
}

#[test]
fn drops_use_matching_carried_models_and_preserve_session_sprite_overrides() {
    let (source, terrain) = crate::presentation::equipment::PreparedBlockItems::narrow_box(20);
    let (replacement_source, replacement) =
        crate::presentation::equipment::PreparedBlockItems::narrow_box(80);
    let (mut icons, stream) = fixture_item(ITEM, 0, 1);
    let mut world = World::new();
    world.init_resource::<DroppedItemScene>();
    world.insert_resource(ChunkTextureAssets::new(Arc::clone(&terrain)));
    let mut publisher = SystemState::<DroppedItemPublisher<'_, '_>>::new(&mut world);
    for (terrain, carried, session, expected) in [
        (&terrain, Some(&source), None, 20),
        (&replacement, Some(&source), None, 5),
        (&replacement, Some(&replacement_source), None, 80),
        (&terrain, Some(&source), Some(90), 90),
        (&terrain, None, None, 5),
        (&terrain, Some(&source), None, 20),
    ] {
        world.insert_resource(ChunkTextureAssets::new(Arc::clone(terrain)));
        install(&mut icons, session, false, 0);
        publisher.get_mut(&mut world).unwrap().publish(
            Some(&icons),
            Some(&stream),
            None,
            (None, None),
            DroppedItemFrame {
                prepared_blocks: carried,
                ..Default::default()
            },
            (None, Default::default()),
        );
        let scene = world.resource::<DroppedItemScene>();
        assert_eq!(scene.instances.len(), 1);
        let model = &scene.models[scene.instances[0].model as usize];
        if session.is_some() {
            let DroppedItemModel::NativeSprite(sprite) = model else {
                panic!("an admitted session sprite must replace stock carried geometry");
            };
            assert_eq!(sprite.rgba8[0], expected);
        } else {
            let DroppedItemModel::Block(block) = model else {
                panic!("the available block source must supply geometry");
            };
            assert_eq!(block.materials[0].0.rgba8[0], expected);
        }
    }
}

/// Installs an original chest texture through the block-entity scene's artwork boundary.
fn chest_artwork(world: &mut World, color: Option<u8>) {
    let mut scene = render_model::BlockEntityScene::default();
    if let Some(color) = color {
        let bytes = assets::encode_block_entity_catalog(
            b"{}",
            render_model::MIN_DYNAMIC_ATLAS_WIDTH,
            64,
            &[color, 40, 60, 255].repeat(render_model::MIN_DYNAMIC_ATLAS_WIDTH as usize * 64),
            &[assets::BlockEntityPlacement {
                name: "textures/entity/chest/normal".into(),
                x: 0,
                y: 0,
                width: 64,
                height: 64,
            }],
        )
        .unwrap();
        scene.install_assets(&assets::RuntimeBlockEntityAssets::decode(&bytes).unwrap());
    }
    world.insert_resource(render_scene::BlockEntitySceneResource(scene));
}

#[test]
fn dedicated_drops_use_current_models_and_preserve_session_artwork_and_fallbacks() {
    let identifier = "minecraft:chest";
    let (mut icons, stream) = fixture_item(identifier, 0, 0);
    let mut world = World::new();
    world.init_resource::<DroppedItemScene>();
    chest_artwork(&mut world, Some(30));
    let mut publisher = SystemState::<DroppedItemPublisher<'_, '_>>::new(&mut world);
    for (art, session, cube) in [
        (Some(30), None, false),
        (Some(80), None, false),
        (Some(80), Some(90), false),
        (Some(80), Some(100), true),
        (None, None, false),
        (Some(30), None, false),
    ] {
        chest_artwork(&mut world, art);
        install_item(&mut icons, identifier, session, cube, 0);
        publisher.get_mut(&mut world).unwrap().publish(
            Some(&icons),
            Some(&stream),
            None,
            (None, None),
            Default::default(),
            (None, Default::default()),
        );
        let scene = world.resource::<DroppedItemScene>();
        assert_eq!(scene.instances.len(), 1);
        let model = &scene.models[scene.instances[0].model as usize];
        match (session, art, cube) {
            (Some(color), _, true) => {
                let DroppedItemModel::Cube(model) = model else {
                    panic!("pack sheets retain priority")
                };
                assert_eq!(model.faces[0][0], color);
            }
            (Some(_), _, false) | (None, None, _) => {
                let DroppedItemModel::NativeSprite(model) = model else {
                    panic!("icons remain a recoverable fallback")
                };
                assert_eq!(model.rgba8[0], session.unwrap_or(20));
            }
            (None, Some(color), _) => {
                let DroppedItemModel::Block(model) = model else {
                    panic!("dedicated items require geometry")
                };
                assert_eq!(model.materials.len(), 1);
                assert_eq!(model.materials[0].0.rgba8[0], color);
                let vertices = render_model::dropped_item::block_mesh(model, 1, 64).unwrap();
                let origin = glam::Vec3::from(vertices[0].position);
                let normal = glam::Vec3::from(vertices[0].normal);
                assert!(
                    vertices
                        .iter()
                        .any(|v| normal.dot(glam::Vec3::from(v.position) - origin).abs() > 0.01)
                );
            }
        }
    }
}
