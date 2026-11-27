use super::*;
use crate::shaderpack::actors::RigSource;
use render_model::hand::HAND_MAP_BACKGROUND_FLAG;
use render_model::{
    ActorGpuInstance, ActorRigGeometrySpan, ActorRigRenderFrame, ActorRigVertexSegments,
    ActorTexturePage, HAND_ITEM_LAYER_FLAG, HAND_OFFHAND_LAYER_FLAG, HandItemAtlas,
};
use render_scene::hand_rig::HandRigFrame;
use std::sync::Arc;

fn fixture_frame() -> HandRigFrame {
    let mut instances = vec![ActorGpuInstance::default(); 6];
    // The arms sentinel and these two regular rig draws all lack held-item role flags.
    instances[0].texture_layer = u32::MAX;
    instances[1].texture_layer = 2;
    instances[2].texture_layer = 1;
    // Ordinary held-item selectors remain independent controls for the catalog routing.
    instances[3].texture_layer = HAND_ITEM_LAYER_FLAG | 1;
    instances[4].texture_layer = HAND_ITEM_LAYER_FLAG | HAND_OFFHAND_LAYER_FLAG | 2;
    instances[5].texture_layer = HAND_ITEM_LAYER_FLAG | HAND_MAP_BACKGROUND_FLAG;
    let page = |width: u16, height: u16, layers: u32, marker: u8| {
        let mut rgba8 =
            Vec::with_capacity(usize::from(width) * usize::from(height) * layers as usize * 4);
        for layer in 0..layers {
            for _ in 0..height {
                for x in 0..width {
                    let pixel = if marker == 79 && layer == 0 {
                        [0, 200, 255, 255]
                    } else if marker == 79 && x >= width / 2 {
                        [184, 48, 40, 255]
                    } else if marker == 79 {
                        [128, 128, 128, 255]
                    } else {
                        let color = marker + layer as u8;
                        [color, color, color, 255]
                    };
                    rgba8.extend_from_slice(&pixel);
                }
            }
        }
        ActorTexturePage {
            width,
            height,
            layers,
            rgba8: rgba8.into(),
            multitexture: false,
        }
    };
    let locations = [
        None,
        Some(render_model::ActorArtworkLocation {
            page: 1,
            layer: 2,
            pose_mode: assets::ActorPoseMode::CompiledLiteral,
            multitexture: None,
        }),
        Some(render_model::ActorArtworkLocation {
            page: 2,
            layer: 1,
            pose_mode: assets::ActorPoseMode::CompiledLiteral,
            multitexture: None,
        }),
        None,
        None,
        None,
    ];
    let mut artwork = render_model::ActorArtworkPages::default();
    artwork.pages = Arc::from([page(7, 9, 3, 31), page(512, 512, 2, 79)]);
    HandRigFrame {
        rig: ActorRigRenderFrame {
            rasters: Arc::from([]),
            frame_generation: 1,
            geometry_revision: 1,
            instances: instances.into(),
            previous_bones: Arc::from([[[0.0; 4]; 3]]),
            current_bones: Arc::from([[[0.0; 4]; 3]]),
            geometry_vertices: ActorRigVertexSegments::from_vertices([ActorRigVertex::default()]),
            geometry_spans: Arc::from([ActorRigGeometrySpan {
                first_vertex: 0,
                vertex_count: 1,
            }]),
            manifest: Arc::from([]),
            maximum_vertex_count: 36,
            rejects: render_model::ActorRigRejects::default(),
        },
        artwork: Some(Arc::new(artwork)),
        draw_locations: Arc::from(locations),
        skin: vec![0; render_model::STANDARD_SKIN_BYTES].into(),
        light: render_model::HandRigLight::default(),
        fov_radians: 1.2,
        revision: 1,
        item_atlases: [
            Some(atlas(21, 17, 3, 113)),
            Some(atlas(23, 19, 4, 157)),
            Some(atlas(25, 11, 1, 199)),
        ],
        item_glint: None,
    }
}

fn atlas(width: u16, height: u16, layers: u32, marker: u8) -> HandItemAtlas {
    HandItemAtlas {
        width,
        height,
        layers,
        rgba8: vec![marker; usize::from(width) * usize::from(height) * layers as usize * 4].into(),
    }
}

#[test]
fn shaderpack_hand_selects_catalog_pages_and_layers_without_changing_skin_or_item_routes() {
    let (device, queue) = wgpu::Device::noop(&Default::default());
    let world = uploaded_world(device, queue);
    let side = render_model::actor_skin_side(
        &world
            .resource::<HandRigScene>()
            .frame
            .as_ref()
            .unwrap()
            .skin,
    )
    .unwrap() as u32;

    let mut selected = Vec::new();
    shaderpack_source::shaderpack_hand(&world)
        .expect("uploaded hand frame is drawable")
        .for_each_instance(&mut |instance| {
            selected.push((
                instance.index,
                instance.texture.texture.width(),
                instance.texture.texture.height(),
                instance.texture.texture.depth_or_array_layers(),
                instance.texture.layer,
            ));
        });

    assert_eq!(
        selected,
        [
            (0, side, side, 1, 0),
            (1, 7, 9, 3, 2),
            (2, 512, 512, 2, 1),
            (3, 21, 17, 3, 1),
            (4, 23, 19, 4, 2),
            (5, 25, 11, 1, 0),
        ],
        "catalog locations select their own page/layer while skin and all held-item routes stay intact"
    );
}

/// Uploads the real hand resources without replacing their production selectors.
fn uploaded_world(device: wgpu::Device, queue: wgpu::Queue) -> World {
    use bevy::ecs::system::RunSystemOnce;
    let device = RenderDevice::from(device);
    let queue = RenderQueue::new(queue);
    let mut world = World::new();
    world.insert_resource(device.clone());
    world.insert_resource(queue.clone());
    world.run_system_once(init_gpu).unwrap();
    let scene = HandRigScene {
        frame: Some(fixture_frame()),
    };
    let frame = scene.frame.as_ref().unwrap();
    let mut gpu = world.remove_resource::<HandRigGpu>().unwrap();
    gpu.raw_views = crate::actor::gpu::ArtworkRawViews(true);
    assert!(upload_geometry(&mut gpu, &device, &queue, frame));
    {
        let mut state =
            bevy::ecs::system::SystemState::<crate::upload_staging::FrameUploads>::new(&mut world);
        let uploads = state.get(&world).unwrap();
        assert!(upload_pose(&mut gpu, &uploads, frame));
    }
    upload_skin(&mut gpu, &device, &queue, frame);
    upload_atlas(&mut gpu, &device, &queue, frame);
    world.insert_resource(scene);
    world.insert_resource(gpu);
    world
}

/// Selected production textures reach the real hand fragment with asymmetric texels.
#[test]
fn catalog_hand_fragment_reads_selected_pages_not_skin_pixels() {
    use crate::gpu_snapshot::{Draw, Gpu, RasterState};
    let Some(native) = Gpu::for_fixture("catalog first-person artwork") else {
        return;
    };
    let world = uploaded_world(native.device.clone(), native.queue.clone());
    let source = crate::shader_safety::from_actor_wesl(
        include_str!("../hand_rig.wesl"),
        "catalog hand fixture",
        render_model::ACTOR_GPU_INSTANCE_WORDS,
        render_model::ACTOR_RIG_VERTEX_WORDS,
    );
    let bevy::shader::Source::Wesl(source) = source.source else {
        panic!("WESL hand")
    };
    let source =
        crate::shader_source::standalone(&source, &["ENCODED_HAND_COLOR", "CATALOG_ROUTING"])
            .replace("@group(1) @binding(0)", "@group(0) @binding(20)")
            .replace("@group(1) @binding(1)", "@group(0) @binding(21)");
    let instances = native.buffer(
        bytemuck::cast_slice(&[ActorGpuInstance::default()]),
        wgpu::BufferUsages::STORAGE,
    );
    let material = native.buffer(
        bytemuck::cast_slice(&[HAND_MATERIAL]),
        wgpu::BufferUsages::UNIFORM,
    );
    let light = native.buffer(
        bytemuck::cast_slice(&[render_model::HandRigLight::default()]),
        wgpu::BufferUsages::UNIFORM,
    );
    let lightmap = native.lightmap(&[1.0; 256 * 4]);
    let sampler = native.device.create_sampler(&Default::default());
    let glint = native.blank_texture_view();
    let mut inspected = 0;
    shaderpack_source::shaderpack_hand(&world)
        .unwrap()
        .for_each_instance(&mut |instance| {
            if instance.index != 1 && instance.index != 2 {
                return;
            }
            let view = instance
                .texture
                .texture
                .create_view(&wgpu::TextureViewDescriptor {
                    dimension: Some(wgpu::TextureViewDimension::D2Array),
                    ..Default::default()
                });
            let layer_selector = native.words(
                &[instance.texture.layer, 1, 0, 0],
                wgpu::BufferUsages::UNIFORM,
            );
            let mut entries = bindings::HandFragmentBindings {
                instances: &instances,
                skins: &view,
                skin_sampler: &sampler,
                material: &material,
                light: &light,
                main_atlas: &view,
                offhand_atlas: &view,
                map_background: &view,
                item_glint: &glint,
                glint_sampler: &sampler,
            }
            .entries()
            .to_vec();
            entries.push(wgpu::BindGroupEntry {
                binding: 15,
                resource: layer_selector.as_entire_binding(),
            });
            entries.push(wgpu::BindGroupEntry {
                binding: 20,
                resource: lightmap.as_entire_binding(),
            });
            let shader = format!(
                "{}\n@vertex fn plate(@builtin(vertex_index) index: u32) -> VertexOutput {{
            let positions = array(vec2(-1.0,-1.0), vec2(3.0,-1.0), vec2(-1.0,3.0));
            var out: VertexOutput; out.position=vec4(positions[index],0.5,1.0);
            out.uv=vec4((positions[index]+vec2(1.0))*0.5,0.0,0.0); out.back_uv=vec2(0.5);
            out.skin_layer={}u; out.valid=1u; out.shade=1.0;
            out.glint_instance=0xffffffffu; out.native_light=vec3(1.0); return out;
        }}",
                source, instance.texture.layer
            );
            let pixels = native.render_with_state(
                &shader,
                "plate",
                &[Draw {
                    fragment: "hand_fragment",
                    vertices: 0..3,
                    bindings: &entries,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_depth: true,
                }],
                RasterState::default(),
            );
            for (x, expected) in if instance.index == 1 {
                [(64, [33, 33, 33, 255]), (192, [33, 33, 33, 255])]
            } else {
                [(64, [128, 128, 128, 255]), (192, [184, 48, 40, 255])]
            } {
                let offset = (128 * 256 + x) * 4;
                assert_eq!(
                    &pixels[offset..offset + 4],
                    &expected,
                    "catalog instance {} at x={x}",
                    instance.index
                );
            }
            inspected += 1;
        });
    assert_eq!(inspected, 2, "both independent pages were rasterized");
}

/// Unchanged pages stay resident, while same-slot replacement cannot reuse stale artwork.
#[test]
fn catalog_hand_retains_warm_textures_and_replaces_changed_page_shapes() {
    let (device, queue) = wgpu::Device::noop(&Default::default());
    let mut world = uploaded_world(device, queue);
    let ids = |world: &World| {
        let mut ids = Vec::new();
        shaderpack_source::shaderpack_hand(world)
            .unwrap()
            .for_each_instance(&mut |instance| {
                ids.push((instance.index, instance.texture.texture.id()));
            });
        ids
    };
    let before = ids(&world);
    let device = world.resource::<RenderDevice>().clone();
    let queue = world.resource::<RenderQueue>().clone();
    let mut gpu = world.remove_resource::<HandRigGpu>().unwrap();
    let mut scene = world.remove_resource::<HandRigScene>().unwrap();
    let frame = scene.frame.as_mut().unwrap();
    let allocations = test_alloc::thread_allocations();
    upload_atlas(&mut gpu, &device, &queue, frame);
    assert_eq!(
        test_alloc::thread_allocations() - allocations,
        0,
        "warm artwork does no heap work"
    );
    world.insert_resource(gpu);
    world.insert_resource(scene);
    assert_eq!(ids(&world), before);
    let mut gpu = world.remove_resource::<HandRigGpu>().unwrap();
    let mut scene = world.remove_resource::<HandRigScene>().unwrap();
    let frame = scene.frame.as_mut().unwrap();
    let pages = &mut Arc::make_mut(frame.artwork.as_mut().unwrap()).pages;
    let page = &mut Arc::make_mut(pages)[0];
    // Equal byte count, but distinct dimensions and pixels, without changing the pose revision.
    page.width = 9;
    page.height = 7;
    page.rgba8 = vec![99; 9 * 7 * 3 * 4].into();
    upload_atlas(&mut gpu, &device, &queue, frame);
    world.insert_resource(gpu);
    world.insert_resource(scene);
    let after = ids(&world);
    assert_eq!(before.len(), after.len());
    for ((index, old), (new_index, new)) in before.iter().zip(after.iter()) {
        assert_eq!(index, new_index);
        assert_eq!(
            old == new,
            *index != 1,
            "only the replaced catalog page changes its texture"
        );
    }
}

/// Malformed or absent catalog references never sample a different resource as fallback.
#[test]
fn catalog_hand_invalid_page_and_layer_do_not_fall_back_to_skin() {
    let (device, queue) = wgpu::Device::noop(&Default::default());
    let mut world = uploaded_world(device, queue);
    let device = world.resource::<RenderDevice>().clone();
    let queue = world.resource::<RenderQueue>().clone();
    let mut gpu = world.remove_resource::<HandRigGpu>().unwrap();
    let mut scene = world.remove_resource::<HandRigScene>().unwrap();
    let frame = scene.frame.as_mut().unwrap();
    let locations = Arc::make_mut(&mut frame.draw_locations);
    locations[1].as_mut().unwrap().page = 99;
    locations[2].as_mut().unwrap().layer = 99;
    upload_atlas(&mut gpu, &device, &queue, frame);
    world.insert_resource(gpu);
    world.insert_resource(scene);
    let mut indices = Vec::new();
    shaderpack_source::shaderpack_hand(&world)
        .unwrap()
        .for_each_instance(&mut |i| indices.push(i.index));
    assert_eq!(indices, [0, 3, 4, 5]);
    let mut gpu = world.remove_resource::<HandRigGpu>().unwrap();
    let mut scene = world.remove_resource::<HandRigScene>().unwrap();
    let frame = scene.frame.as_mut().unwrap();
    frame.artwork = None;
    upload_atlas(&mut gpu, &device, &queue, frame);
    world.insert_resource(gpu);
    world.insert_resource(scene);
    let mut indices = Vec::new();
    shaderpack_source::shaderpack_hand(&world)
        .unwrap()
        .for_each_instance(&mut |i| indices.push(i.index));
    assert_eq!(indices, [0, 3, 4, 5]);
}
