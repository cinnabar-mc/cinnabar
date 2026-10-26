//! First-person lighting through the production vertex and fragment entry points.
use crate::{
    gpu_snapshot::{Draw, Gpu},
    hand_rig_bindings::{HandFragmentBindings, hand_artwork_format},
    shader_safety, shader_source,
};
use bevy::math::{Mat4, Vec3};
use render_model::ActorRigVertex;
use render_model::{ActorGpuInstance, ActorRigGeometrySpan, HandItemAlphaMode, HandRigLight};

/// Resolves packed storage constants and binds the real lightmap beside the hand resources.
fn source() -> String {
    source_with_definitions(&[])
}

/// Resolves the production hand shader for one attachment color space.
fn source_with_definitions(definitions: &[&str]) -> String {
    let shader = shader_safety::from_actor_wesl(
        include_str!("../../src/hand_rig.wesl"),
        "hand_lighting.wesl",
        render_model::ACTOR_GPU_INSTANCE_WORDS,
        render_model::ACTOR_RIG_VERTEX_WORDS,
    );
    let bevy::shader::Source::Wesl(source) = shader.source else {
        panic!("WGSL shader")
    };
    shader_source::standalone(&source, definitions)
        .replace("@group(1) @binding(0)", "@group(0) @binding(20)")
        .replace("@group(1) @binding(1)", "@group(0) @binding(21)")
}

#[test]
fn native_hand_samples_preserve_unorm_source_bytes() {
    let Some(gpu) = Gpu::for_fixture("native hand source texels") else {
        return;
    };
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("original hand texel byte ramp"),
        size: wgpu::Extent3d {
            width: 16,
            height: 16,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: hand_artwork_format(&HandRigLight::default()),
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[wgpu::TextureFormat::Rgba8Unorm],
    });
    let pixels: Vec<u8> = (0..=255_u16)
        .flat_map(|value| {
            [
                value as u8,
                (255 - value) as u8,
                (value * 37 % 256) as u8,
                value as u8,
            ]
        })
        .collect();
    gpu.queue.write_texture(
        texture.as_image_copy(),
        &pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(64),
            rows_per_image: Some(16),
        },
        texture.size(),
    );
    let sampled = texture.create_view(&Default::default());
    let reference = texture.create_view(&wgpu::TextureViewDescriptor {
        format: Some(wgpu::TextureFormat::Rgba8Unorm),
        ..Default::default()
    });
    let shader = format!(
        "{}\n@group(0) @binding(22) var decoded_texels: texture_2d<f32>; @group(0) @binding(23) var native_texels: texture_2d<f32>; @group(0) @binding(24) var<storage, read_write> result: array<vec4<u32>>; @compute @workgroup_size(1) fn witness(@builtin(global_invocation_id) id: vec3<u32>) {{ let point = vec2<i32>(i32(id.x % 16u), i32(id.x / 16u)); result[id.x*2u] = bitcast<vec4<u32>>(hand_gamma_sample(textureLoad(decoded_texels, point, 0), true)); result[id.x*2u+1u] = bitcast<vec4<u32>>(textureLoad(native_texels, point, 0)); }}",
        source()
    );
    let actual = gpu.compute_readback(
        &shader,
        &[
            wgpu::BindGroupEntry {
                binding: 22,
                resource: wgpu::BindingResource::TextureView(&sampled),
            },
            wgpu::BindGroupEntry {
                binding: 23,
                resource: wgpu::BindingResource::TextureView(&reference),
            },
        ],
        24,
        512,
        256,
    );
    for (value, pair) in actual.chunks_exact(2).enumerate() {
        assert_eq!(pair[0], pair[1], "source byte {value}");
    }
}

#[test]
fn translucent_hand_blends_in_gamma_over_existing_scene_bytes() {
    let Some(gpu) = Gpu::for_fixture("translucent hand gamma composition") else {
        return;
    };
    assert!(crate::hand_rig_bindings::hand_uses_encoded_color(
        &HandRigLight::default(),
        false
    ));
    let sampler = gpu.device.create_sampler(&Default::default());
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("translucent carried texel"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    gpu.queue.write_texture(
        texture.as_image_copy(),
        &[64, 128, 192, 128],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: Some(1),
        },
        texture.size(),
    );
    let atlas = texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    let glint = gpu.blank_texture_view();
    let instances = gpu.buffer(
        bytemuck::cast_slice(&[ActorGpuInstance::default()]),
        wgpu::BufferUsages::STORAGE,
    );
    let item = render_model::HAND_ITEM_LAYER_FLAG;
    let blend = HandItemAlphaMode::Blend.texture_layer_flag();
    let cutout = HandItemAlphaMode::Cutout.texture_layer_flag();
    let offhand = render_model::HAND_OFFHAND_LAYER_FLAG;
    let material = gpu.words(
        &[
            item,
            offhand,
            blend,
            cutout,
            !(item | offhand | blend | cutout),
            0,
            0,
            0,
        ],
        wgpu::BufferUsages::UNIFORM,
    );
    let light = gpu.buffer(
        bytemuck::cast_slice(&[HandRigLight::default()]),
        wgpu::BufferUsages::UNIFORM,
    );
    let lightmap = gpu.lightmap(&[1.0; 256 * 4]);
    let mut bindings = HandFragmentBindings {
        instances: &instances,
        skins: &atlas,
        skin_sampler: &sampler,
        material: &material,
        light: &light,
        main_atlas: &atlas,
        offhand_atlas: &atlas,
        map_background: &atlas,
        item_glint: &glint,
        glint_sampler: &sampler,
    }
    .entries()
    .to_vec();
    bindings.push(wgpu::BindGroupEntry {
        binding: 20,
        resource: lightmap.as_entire_binding(),
    });
    let source = format!(
        "{}\n@vertex fn plate(@builtin(vertex_index) index: u32) -> VertexOutput {{
        let positions = array(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
        var out: VertexOutput;
        out.position = vec4(positions[index], 0.5, 1.0);
        out.uv = vec4(0.5, 0.5, 0.0, 0.0);
        out.back_uv = vec2(0.5);
        out.skin_layer = {layer}u;
        out.valid = 1u; out.shade = 1.0;
        out.glint_instance = 0xffffffffu; out.native_light = vec3(1.0);
        return out;
    }}",
        source_with_definitions(&["ENCODED_HAND_COLOR"]),
        layer = item | blend
    );
    let pixels = gpu.render_with_state(
        &source,
        "plate",
        &[Draw {
            fragment: "hand_fragment",
            vertices: 0..3,
            bindings: &bindings,
            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
            write_depth: true,
        }],
        crate::gpu_snapshot::RasterState {
            clear_color: wgpu::Color {
                r: 192.0 / 255.0,
                g: 160.0 / 255.0,
                b: 128.0 / 255.0,
                a: 1.0,
            },
            ..Default::default()
        },
    );
    let center = (128 * 256 + 128) * 4;
    assert_eq!(&pixels[center..center + 4], &[128, 144, 160, 255]);
    let reverse_source = format!(
        "{source}\n@vertex fn plate_reverse(@builtin(vertex_index) index: u32) -> VertexOutput {{
        let positions = array(vec2(-1.0, -1.0), vec2(-1.0, 3.0), vec2(3.0, -1.0));
        var out: VertexOutput;
        out.position = vec4(positions[index], 0.5, 1.0);
        out.uv = vec4(0.5, 0.5, 0.0, 0.0);
        out.back_uv = vec2(-1.0e9);
        out.skin_layer = {layer}u;
        out.valid = 1u; out.shade = 1.0;
        out.glint_instance = 0xffffffffu; out.native_light = vec3(1.0);
        return out;
    }}",
        layer = item | cutout
    );
    let reverse = gpu.render_with_state(
        &reverse_source,
        "plate_reverse",
        &[Draw {
            fragment: "hand_fragment",
            vertices: 0..3,
            bindings: &bindings,
            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
            write_depth: true,
        }],
        crate::gpu_snapshot::RasterState {
            clear_color: wgpu::Color {
                r: 192.0 / 255.0,
                g: 160.0 / 255.0,
                b: 128.0 / 255.0,
                a: 1.0,
            },
            ..Default::default()
        },
    );
    assert_eq!(
        &reverse[center..center + 4],
        &[192, 160, 128, 255],
        "one-sided cutout surfaces leave the background visible from behind"
    );
}

#[test]
fn bedrock_and_java_hand_light_normals_and_compose_texture_in_gamma() {
    let Some(gpu) = Gpu::for_fixture("Bedrock and Java first-person lighting") else {
        return;
    };
    let identity: [[f32; 4]; 3] = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ];
    let projection = gpu.buffer(
        &bevy::math::Mat4::IDENTITY.to_cols_array(),
        wgpu::BufferUsages::UNIFORM,
    );
    let bones = gpu.buffer(bytemuck::cast_slice(&identity), wgpu::BufferUsages::STORAGE);
    let spans = gpu.buffer(
        bytemuck::cast_slice(&[ActorRigGeometrySpan {
            first_vertex: 0,
            vertex_count: 6,
        }]),
        wgpu::BufferUsages::STORAGE,
    );
    let blend = HandItemAlphaMode::Blend.texture_layer_flag();
    let cutout = HandItemAlphaMode::Cutout.texture_layer_flag();
    let material = gpu.words(
        &[
            render_model::HAND_ITEM_LAYER_FLAG,
            render_model::HAND_OFFHAND_LAYER_FLAG,
            blend,
            cutout,
            !(render_model::HAND_ITEM_LAYER_FLAG
                | render_model::HAND_OFFHAND_LAYER_FLAG
                | blend
                | cutout),
            0,
            0,
            0,
        ],
        wgpu::BufferUsages::UNIFORM,
    );
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("original lighting test texel"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[wgpu::TextureFormat::Rgba8Unorm],
    });
    gpu.queue.write_texture(
        texture.as_image_copy(),
        &[128, 227, 255, 255],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: Some(1),
        },
        texture.size(),
    );
    let sampler = gpu.device.create_sampler(&Default::default());
    let glint = gpu.blank_texture_view();
    for (layer, basis, stretch) in [
        (0, Mat4::IDENTITY, 1.0),
        (render_model::HAND_ITEM_LAYER_FLAG, Mat4::IDENTITY, 1.0),
        (
            render_model::HAND_ITEM_LAYER_FLAG,
            Mat4::from_rotation_x(std::f32::consts::FRAC_PI_2),
            1.2,
        ),
    ] {
        let model = Mat4::from_scale(Vec3::new(1.0, 1.0, stretch)) * basis;
        let rows = model.transpose().to_cols_array_2d();
        let instances = gpu.buffer(
            bytemuck::cast_slice(&[ActorGpuInstance {
                world_from_actor: [rows[0], rows[1], rows[2]],
                texture_layer: layer,
                ..Default::default()
            }]),
            wgpu::BufferUsages::STORAGE,
        );
        for (java, varying_normals, normal, world_light, camera_angle, emission, expected) in [
            (false, false, [0.0, -1.0, 0.0], 1.0_f32, 0.0, 0, 57.6),
            (false, false, [0.0, 1.0, 0.0], 1.0, 0.0, 0, 128.0),
            (false, false, [1.0, 0.0, 0.0], 1.0, 0.0, 0, 80.0),
            (false, false, [0.0, 0.0, 1.0], 1.0, 0.0, 0, 105.6),
            (
                false,
                false,
                [0.0, 0.0, 1.0],
                1.0,
                std::f32::consts::FRAC_PI_2,
                0,
                57.6,
            ),
            (false, false, [0.0, -1.0, 0.0], 0.5, 0.0, 15, 57.6),
            (true, false, [0.0, -1.0, 0.0], 1.0, 0.0, 0, 51.2),
            (true, false, [0.0, 1.0, 0.0], 1.0, 0.0, 0, 128.0),
            (
                true,
                false,
                [1.0, 0.0, 0.0],
                1.0,
                0.0,
                0,
                128.0 * (0.4 + stretch * 0.6 * 0.2 / 1.53_f32.sqrt()),
            ),
            (true, false, [0.0, 0.0, 1.0], 1.0, 0.0, 0, 94.6614),
            (true, false, [0.0, 1.0, 0.0], 0.5, 0.0, 0, 64.0),
            (false, true, [0.0, 1.0, 0.0], 1.0, 0.0, 0, 92.9375),
            (
                false,
                false,
                [0.0, 1.0, 0.0],
                252.0 / 255.0,
                std::f32::consts::FRAC_PI_4,
                0,
                122.6303,
            ),
        ] {
            let vertices = [
                (-1.0, -1.0),
                (1.0, -1.0),
                (1.0, 1.0),
                (-1.0, -1.0),
                (1.0, 1.0),
                (-1.0, 1.0),
            ]
            .map(|(x, y)| ActorRigVertex {
                position: model
                    .inverse()
                    .transform_point3(Vec3::new(x, y, 0.5))
                    .to_array(),
                normal: basis
                    .inverse()
                    .transform_vector3(if varying_normals {
                        if x < 0.0 { Vec3::NEG_Y } else { Vec3::Y }
                    } else {
                        Vec3::from_array(normal)
                    })
                    .to_array(),
                uv: [0.5; 2],
                back_uv: [0.5; 2],
                ..Default::default()
            });
            let vertices = gpu.buffer(bytemuck::cast_slice(&vertices), wgpu::BufferUsages::STORAGE);
            let mut light = HandRigLight::default();
            if java {
                light = light.with_java_lighting(Mat4::IDENTITY);
                light.java_normal_axes = [basis
                    .inverse()
                    .transform_vector3(Vec3::Z)
                    .extend(0.0)
                    .to_array(); 3];
            }
            if camera_angle != 0.0 {
                light = light.with_bedrock_lighting(Mat4::from_rotation_x(camera_angle));
            }
            light.emitted_light = [0, emission, emission, 0];
            let atlas = texture.create_view(&wgpu::TextureViewDescriptor {
                format: Some(hand_artwork_format(&light)),
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                ..Default::default()
            });
            let light = gpu.buffer(bytemuck::cast_slice(&[light]), wgpu::BufferUsages::UNIFORM);
            let mut lightmap = [[world_light, world_light, world_light, 1.0]; 256];
            if emission != 0 {
                lightmap[emission as usize] = [1.0; 4];
            }
            let lightmap = gpu.lightmap(bytemuck::cast_slice(&lightmap));
            let expected = if emission != 0 && layer == 0 {
                expected * world_light
            } else {
                expected
            };
            let buffers = [
                (0, &projection),
                (2, &vertices),
                (3, &spans),
                (4, &bones),
                (5, &bones),
                (20, &lightmap),
            ];
            let mut bindings: Vec<_> = buffers
                .into_iter()
                .map(|(binding, buffer)| wgpu::BindGroupEntry {
                    binding,
                    resource: buffer.as_entire_binding(),
                })
                .collect();
            bindings.extend(
                HandFragmentBindings {
                    instances: &instances,
                    skins: &atlas,
                    skin_sampler: &sampler,
                    material: &material,
                    light: &light,
                    main_atlas: &atlas,
                    offhand_atlas: &atlas,
                    map_background: &atlas,
                    item_glint: &glint,
                    glint_sampler: &sampler,
                }
                .entries(),
            );
            let pixels = gpu.render_srgb(
                &source(),
                "hand_vertex",
                &[Draw {
                    fragment: "hand_fragment",
                    vertices: 0..6,
                    bindings: &bindings,
                    blend: None,
                    write_depth: true,
                }],
            );
            let center = (128 * 256 + 128) * 4;
            let observed = pixels[center];
            if camera_angle == std::f32::consts::FRAC_PI_4 {
                assert_eq!(
                    pixels[center + 1],
                    217,
                    "opaque gamma output rounds before sRGB storage"
                );
            }
            assert!(
                (f32::from(observed) - expected).abs() <= 1.0,
                "Java={java}, varying={varying_normals}, layer={layer}, stretch={stretch}, normal={normal:?}, world={world_light}: expected {expected}, got {observed}"
            );
        }
    }
}
