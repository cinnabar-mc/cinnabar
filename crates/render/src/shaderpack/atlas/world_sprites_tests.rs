use super::super::gpu_tests::read_level;
use super::super::pixels::build_images;
use super::super::plan::plan_atlas;
use super::super::tests::{fixture, slot_rect};
use super::super::upload::upload_atlas;
use super::super::{WORLD_MATERIAL_SPRITES, animation_sprite_slot, world_material_sprite_slot};
use super::*;

/// The plan reserves one block-sized sprite per world material slot, with an empty remap
/// entry, between the static and the animation sprites.
#[test]
fn the_atlas_reserves_an_unpublished_sprite_per_world_material_slot() {
    let mut fixture = fixture();
    fixture.world_sprites = WORLD_MATERIAL_SPRITES;
    let plan = fixture.with_inputs(None, |inputs| plan_atlas(inputs, 4096).unwrap());
    assert_eq!(plan.world_sprites.len(), WORLD_MATERIAL_SPRITES as usize);
    for (slot, rect) in plan.world_sprites.iter().enumerate() {
        assert_eq!(rect.side, assets::TILE_SIZE, "slot {slot}");
        let at = world_material_sprite_slot(slot as u32) as usize * 2;
        assert_eq!(plan.remap[at..at + 2], [0, 0], "slot {slot} starts hidden");
    }
    assert!(animation_sprite_slot(0) > world_material_sprite_slot(WORLD_MATERIAL_SPRITES - 1));
    // Animation sprites keep their own squares after the reserved slots.
    assert_eq!(slot_rect(&plan, animation_sprite_slot(0)).side, 16);
}

/// A guest that emits half of a saturated red.
const GLOW: &str = "fn shade(input: WorldMaterialInput) -> Surface { return Surface(vec3<f32>(0.5, 0.0, 0.0), 1.0, vec3<f32>(0.5, 0.0, 0.0)); }";

/// The sprite pipeline and bind group of `GLOW`, built from the production sprite stage.
fn glow_sprite(gpu: &crate::gpu_snapshot::Gpu) -> (wgpu::RenderPipeline, wgpu::BindGroup) {
    let material = mod_render::world_material::compose(GLOW, 0).unwrap();
    let module = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("world material sprite"),
            source: wgpu::ShaderSource::Wgsl(
                format!("{}\n{SPRITE_SOURCE}", material.library(0)).into(),
            ),
        });
    let layout = gpu
        .device
        .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
    let pipeline_layout = gpu
        .device
        .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
    let targets = [
        super::super::upload::COLOR_STORAGE_FORMAT,
        PBR_FORMAT,
        PBR_FORMAT,
    ]
    .map(|format| Some(format.into()));
    let pipeline = gpu
        .device
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("world_sprite_vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("world_sprite_fragment"),
                compilation_options: Default::default(),
                targets: &targets,
            }),
            multiview_mask: None,
            cache: None,
        });
    let frame = gpu.buffer(
        &mod_render::world_material::frame_uniform(0.0, 1, &[]),
        wgpu::BufferUsages::UNIFORM,
    );
    let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: frame.as_entire_binding(),
        }],
    });
    (pipeline, group)
}

/// The remap entry of world material slot `slot`.
fn remap_entry(gpu: &crate::gpu_snapshot::Gpu, remap: &wgpu::Buffer, slot: u32) -> [u32; 2] {
    let staging = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 8,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(
        remap,
        u64::from(world_material_sprite_slot(slot)) * 8,
        &staging,
        0,
        8,
    );
    gpu.queue.submit([encoder.finish()]);
    staging
        .slice(..)
        .map_async(wgpu::MapMode::Read, |result| result.unwrap());
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    let mapped = staging.slice(..).get_mapped_range().unwrap();
    *bytemuck::from_bytes::<[u32; 2]>(&mapped)
}

#[test]
fn a_world_material_renders_its_sprite_and_publishes_it_until_revoked() {
    let Some(gpu) = crate::gpu_snapshot::Gpu::for_fixture("shader-pack world material sprite")
    else {
        return;
    };
    let mut fixture = fixture();
    fixture.world_sprites = WORLD_MATERIAL_SPRITES;
    let (plan, images) = fixture.with_inputs(None, |inputs| {
        let plan = plan_atlas(inputs, gpu.device.limits().max_texture_dimension_2d).unwrap();
        let images = build_images(inputs, &plan, &|| false).unwrap();
        (plan, images)
    });
    let uploaded = upload_atlas(&gpu.device, &gpu.queue, &plan, &images, &|| false).unwrap();
    let mut sprites =
        WorldMaterialSprites::new(&uploaded, plan.world_sprites.clone(), plan.mip_levels);
    let (pipeline, group) = glow_sprite(&gpu);
    let slot = 5;
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    assert!(sprites.encode(
        &gpu.queue,
        &mut encoder,
        &[ReadySprite {
            slot,
            key: 7,
            pipeline: &pipeline,
            group: &group,
        }],
    ));
    gpu.queue.submit([encoder.finish()]);
    let rect = plan.world_sprites[slot];
    for level in 0..plan.mip_levels {
        let colour = read_level(&gpu, &uploaded.atlas.color, level).read(level, rect);
        assert!(
            colour
                .as_chunks::<4>()
                .0
                .iter()
                .all(|texel| *texel == [255, 0, 0, 255]),
            "the guest's colour and emission fill mip {level}"
        );
        let pbr = uploaded.atlas.pbr.as_ref().unwrap();
        let specular = read_level(&gpu, &pbr[1], level).read(level, rect);
        assert!(
            specular
                .as_chunks::<4>()
                .0
                .iter()
                .all(|texel| texel[3] == 127),
            "half emission is LabPBR emissive 127 at mip {level}"
        );
    }
    assert_eq!(
        remap_entry(&gpu, &uploaded.remap, slot as u32),
        [rect.x | (rect.y << 16), rect.side | (rect.side << 16)],
        "the sprite is published once rendered"
    );
    let revision = sprites.revision;
    assert!(revision > 0);
    // Rendering the same material again publishes nothing new.
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    sprites.encode(
        &gpu.queue,
        &mut encoder,
        &[ReadySprite {
            slot,
            key: 7,
            pipeline: &pipeline,
            group: &group,
        }],
    );
    gpu.queue.submit([encoder.finish()]);
    assert_eq!(sprites.revision, revision);
    // Revocation clears the entry, so bound faces return to their Bedrock sprite.
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    assert!(!sprites.encode(&gpu.queue, &mut encoder, &[]));
    gpu.queue.submit([encoder.finish()]);
    assert_eq!(remap_entry(&gpu, &uploaded.remap, slot as u32), [0, 0]);
    assert!(sprites.revision > revision);
}
