use bevy::render::render_resource::Specializer;

use crate::actor::test_material::TestMaterial;
use crate::{actor_raster, gpu_snapshot};
use assets::EntityRenderMaterial;
use bevy::{prelude::Msaa, shader::ShaderDefVal};

use super::super::{
    ActorPipelineKey, ActorPipelineSpecializer, actor_bind_group_layout, actor_pipeline_descriptor,
};

fn center(frame: &[u8]) -> &[u8] {
    let pixel = (gpu_snapshot::SNAPSHOT_SIDE as usize / 2 * gpu_snapshot::SNAPSHOT_SIDE as usize
        + gpu_snapshot::SNAPSHOT_SIDE as usize / 2)
        * 4;
    &frame[pixel..pixel + 4]
}

#[test]
fn authored_actor_opacity_multiplier_preserves_background() {
    let Some(gpu) =
        gpu_snapshot::Gpu::for_fixture("authored_actor_opacity_multiplier_preserves_background")
    else {
        return;
    };
    let material = TestMaterial {
        blend: true,
        ..Default::default()
    }
    .material(EntityRenderMaterial::Default);
    let plane = actor_raster::cube([16, 0, 16], true, false);
    let draw = |alpha: u8| {
        actor_raster::raster_material_with_overlay(
            &gpu,
            &plane,
            material,
            false,
            [[120, 160, 200, 255]; 2],
            false,
            true,
            0,
            0,
            render_model::pack_overlay_rgba8([1.0, 1.0, 1.0, f32::from(alpha) / 255.0]),
        )
    };
    let full = draw(255);
    let background = actor_raster::raster_material_with_overlay(
        &gpu,
        &plane,
        material,
        false,
        [[0; 4]; 2],
        false,
        true,
        0,
        0,
        0,
    );
    for alpha in [0, 77] {
        let frame = draw(alpha);
        let opacity = f32::from(alpha) / 255.0;
        for channel in 0..3 {
            let expected = f32::from(center(&full)[channel]) * opacity
                + f32::from(center(&background)[channel]) * (1.0 - opacity);
            assert!(
                (f32::from(center(&frame)[channel]) - expected).abs() <= 2.0,
                "controller alpha {alpha} must weight the source: got {}, expected {expected}",
                center(&frame)[channel]
            );
        }
    }
}

#[test]
fn actor_ignoring_lightmap_keeps_directional_shading_and_authored_multiplier() {
    let Some(gpu) = gpu_snapshot::Gpu::for_fixture(
        "actor_ignoring_lightmap_keeps_directional_shading_and_authored_multiplier",
    ) else {
        return;
    };
    let material = render_model::ActorMaterial {
        light_color_multiplier: 0.5,
        ..TestMaterial {
            disable_overlay: true,
            ..Default::default()
        }
        .material(EntityRenderMaterial::Default)
    };
    for normal in [[0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]] {
        let mut plane = actor_raster::cube([16, 0, 16], true, false);
        for vertex in &mut plane {
            vertex.normal = normal;
        }
        let frame = actor_raster::raster_material_with_overlay(
            &gpu,
            &plane,
            material,
            false,
            [[200, 120, 40, 255]; 2],
            false,
            true,
            render_model::pack_actor_light_without_lightmap(),
            render_model::pack_overlay_rgba8([1.0; 4]),
            0,
        );
        let shade = ::render_api::fancy_actor_shade(normal, 0.0);
        for (actual, source) in center(&frame)[..3].iter().zip([200, 120, 40]) {
            let expected = (source as f32 * 0.5 * shade).round() as i32;
            assert!(
                (i32::from(*actual) - expected).abs() <= 1,
                "ignoring the lightmap preserves face shading: {normal:?}, {actual} != {expected}"
            );
        }
    }
}

#[test]
fn additive_actor_without_overlay_keeps_authored_rgb_and_light_multiplier() {
    let Some(gpu) = gpu_snapshot::Gpu::for_fixture(
        "additive_actor_without_overlay_keeps_authored_rgb_and_light_multiplier",
    ) else {
        return;
    };
    let plane = actor_raster::cube([16, 0, 16], true, false);
    let source = [32, 100, 200, 255];
    for disable_overlay in [false, true] {
        let material = render_model::ActorMaterial {
            light_color_multiplier: 0.5,
            ..TestMaterial {
                cull: false,
                blend: true,
                additive: true,
                disable_overlay,
                ..Default::default()
            }
            .material(EntityRenderMaterial::Default)
        };
        for light in [
            0,
            render_model::pack_actor_light(15, 15),
            render_model::pack_actor_light_without_lightmap(),
        ] {
            let clear = actor_raster::raster_material_with_overlay(
                &gpu,
                &plane,
                material,
                false,
                [[0; 4]; 2],
                false,
                true,
                light,
                0,
                0,
            );
            for overlay in [0, render_model::pack_overlay_rgba8([1.0; 4])] {
                let frame = actor_raster::raster_material_with_overlay(
                    &gpu,
                    &plane,
                    material,
                    false,
                    [source; 2],
                    false,
                    true,
                    light,
                    overlay,
                    0,
                );
                let admitted = !disable_overlay && overlay != 0;
                let shade = if light == 0 {
                    1.0
                } else {
                    ::render_api::fancy_actor_shade(
                        [0.0, 1.0, 0.0],
                        if admitted { 1.0 } else { 0.0 },
                    )
                };
                for ((actual, destination), source) in center(&frame)[..3]
                    .iter()
                    .zip(&center(&clear)[..3])
                    .zip(source)
                {
                    let color = if admitted { 255 } else { source };
                    let expected = i32::from(*destination)
                        + (f32::from(color) * material.light_color_multiplier * shade).round()
                            as i32;
                    assert!(
                        (i32::from(*actual) - expected).abs() <= 1,
                        "overlay admission controls color and lighting: disabled={disable_overlay}, light={light}, overlay={overlay}, {actual} != {expected}"
                    );
                }
            }
        }
    }
}

#[test]
fn emissive_alpha_test_keeps_colored_zero_alpha_and_weights_only_lighting() {
    let Some(gpu) = gpu_snapshot::Gpu::for_fixture(
        "emissive_alpha_test_keeps_colored_zero_alpha_and_weights_only_lighting",
    ) else {
        return;
    };
    let plane = actor_raster::cube([16, 0, 16], true, false);
    let material = render_model::ActorMaterial {
        light_color_multiplier: 0.25,
        ..TestMaterial {
            alpha_test: true,
            cull: false,
            emissive: true,
            ..Default::default()
        }
        .material(EntityRenderMaterial::Default)
    };
    for alpha in [0, 255] {
        let frame = actor_raster::raster_material_lighting(
            &gpu,
            &plane,
            material,
            false,
            [[200, 120, 40, alpha]; 2],
            true,
            false,
            0,
        );
        let expected = if alpha == 0 {
            [200, 120, 40]
        } else {
            [50, 30, 10]
        };
        for (actual, expected) in center(&frame)[..3].iter().zip(expected) {
            assert!((i32::from(*actual) - expected).abs() <= 1);
        }
        assert_eq!(center(&frame)[3], alpha);
    }
    let clear = actor_raster::raster_material_target(
        &gpu,
        &plane,
        material,
        false,
        [[0; 4]; 2],
        true,
        false,
    );
    let ordinary = actor_raster::raster_material_target(
        &gpu,
        &plane,
        TestMaterial {
            alpha_test: true,
            ..Default::default()
        }
        .material(EntityRenderMaterial::Default),
        false,
        [[200, 120, 40, 0]; 2],
        true,
        false,
    );
    assert_eq!(center(&ordinary), center(&clear));
}

#[test]
fn additive_actor_material_adds_rgb_even_when_texture_alpha_is_zero() {
    let Some(gpu) = gpu_snapshot::Gpu::for_fixture(
        "additive_actor_material_adds_rgb_even_when_texture_alpha_is_zero",
    ) else {
        return;
    };
    let material = TestMaterial {
        alpha_test: true,
        cull: false,
        blend: true,
        additive: true,
        emissive: true,
        ..Default::default()
    }
    .material(EntityRenderMaterial::Default);
    let mut descriptor = actor_pipeline_descriptor(actor_bind_group_layout());
    ActorPipelineSpecializer
        .specialize(
            ActorPipelineKey {
                msaa: Msaa::Off,
                hdr: false,
                enhanced: false,
                material: material.gpu_word(),
                raster: material.raster(),
            },
            &mut descriptor,
        )
        .unwrap();
    let fragment = descriptor.fragment.unwrap();
    let target = fragment.targets[0].as_ref().unwrap();
    let blend = target.blend.unwrap();
    assert_eq!(
        blend.color.src_factor,
        bevy::render::render_resource::BlendFactor::One
    );
    assert_eq!(
        blend.color.dst_factor,
        bevy::render::render_resource::BlendFactor::One
    );
    let srgb = target.format.is_srgb();
    let gamma = fragment.shader_defs.iter().any(
        |define| matches!(define, ShaderDefVal::Bool(name, true) if name == "ACTOR_GAMMA_BLEND"),
    );
    assert!(
        gamma && !srgb,
        "ordinary additive entities compose gamma RGB"
    );
    let plane = actor_raster::cube([16, 0, 16], true, false);
    let clear = actor_raster::raster_material_target(
        &gpu,
        &plane,
        material,
        false,
        [[0; 4]; 2],
        srgb,
        gamma,
    );
    let source = [32, 64, 96, 0];
    let result = actor_raster::raster_material_target(
        &gpu,
        &plane,
        material,
        false,
        [source; 2],
        srgb,
        gamma,
    );
    for ((actual, destination), source) in center(&result)[..3]
        .iter()
        .zip(&center(&clear)[..3])
        .zip(source)
    {
        let expected = i32::from(*destination) + i32::from(source);
        assert!((i32::from(*actual) - expected).abs() <= 1);
    }
    assert_eq!(center(&result)[3], center(&clear)[3]);
}

#[test]
fn alpha_weighted_additive_actor_preserves_destination_and_fades_source() {
    let Some(gpu) = gpu_snapshot::Gpu::for_fixture(
        "alpha_weighted_additive_actor_preserves_destination_and_fades_source",
    ) else {
        return;
    };
    let material = TestMaterial {
        cull: false,
        blend: true,
        depth_write: false,
        additive: true,
        additive_alpha: true,
        emissive: true,
        ..Default::default()
    }
    .material(EntityRenderMaterial::Default);
    let mut descriptor = actor_pipeline_descriptor(actor_bind_group_layout());
    ActorPipelineSpecializer
        .specialize(
            ActorPipelineKey {
                msaa: Msaa::Off,
                hdr: false,
                enhanced: false,
                material: material.gpu_word(),
                raster: material.raster(),
            },
            &mut descriptor,
        )
        .unwrap();
    assert_eq!(
        descriptor.depth_stencil.unwrap().depth_write_enabled,
        Some(false)
    );
    let fragment = descriptor.fragment.unwrap();
    let target = fragment.targets[0].as_ref().unwrap();
    let blend = target.blend.unwrap();
    assert_eq!(
        blend.color.src_factor,
        bevy::render::render_resource::BlendFactor::SrcAlpha
    );
    assert_eq!(
        blend.color.dst_factor,
        bevy::render::render_resource::BlendFactor::One
    );
    let srgb = target.format.is_srgb();
    let gamma = fragment.shader_defs.iter().any(
        |define| matches!(define, ShaderDefVal::Bool(name, true) if name == "ACTOR_GAMMA_BLEND"),
    );
    let plane = actor_raster::cube([16, 0, 16], true, false);
    let clear = actor_raster::raster_material_target(
        &gpu,
        &plane,
        material,
        false,
        [[0; 4]; 2],
        srgb,
        gamma,
    );
    for alpha in [0, 64, 255] {
        let source = [32, 64, 96, alpha];
        let result = actor_raster::raster_material_target(
            &gpu,
            &plane,
            material,
            false,
            [source; 2],
            srgb,
            gamma,
        );
        for ((actual, destination), source) in center(&result)[..3]
            .iter()
            .zip(&center(&clear)[..3])
            .zip(source)
        {
            let expected =
                i32::from(*destination) + (i32::from(source) * i32::from(alpha) + 127) / 255;
            assert!((i32::from(*actual) - expected).abs() <= 1);
        }
    }
}

#[test]
fn actor_material_black_plate_blends_encoded_destination_channels() {
    let Some(gpu) = gpu_snapshot::Gpu::for_fixture(
        "actor_material_black_plate_blends_encoded_destination_channels",
    ) else {
        return;
    };
    let material = TestMaterial {
        alpha_test: true,
        cull: false,
        blend: true,
        ..Default::default()
    }
    .material(EntityRenderMaterial::Default);
    let mut descriptor = actor_pipeline_descriptor(actor_bind_group_layout());
    ActorPipelineSpecializer
        .specialize(
            ActorPipelineKey {
                msaa: Msaa::Off,
                hdr: false,
                enhanced: false,
                material: material.gpu_word(),
                raster: material.raster(),
            },
            &mut descriptor,
        )
        .unwrap();
    let fragment = descriptor.fragment.unwrap();
    let srgb = fragment.targets[0].as_ref().unwrap().format.is_srgb();
    let gamma = fragment.shader_defs.iter().any(
        |define| matches!(define, ShaderDefVal::Bool(name, true) if name == "ACTOR_GAMMA_BLEND"),
    );
    let plane = actor_raster::cube([16, 0, 16], true, false);
    let clear = actor_raster::raster_material_target(
        &gpu,
        &plane,
        render_model::ActorMaterial {
            kind: EntityRenderMaterial::Dragon,
            ..Default::default()
        },
        false,
        [[0; 4]; 2],
        srgb,
        gamma,
    );
    let plate_alpha = 153;
    let plate = actor_raster::raster_material_target(
        &gpu,
        &plane,
        material,
        false,
        [[0, 0, 0, plate_alpha]; 2],
        srgb,
        gamma,
    );
    let pixel = (gpu_snapshot::SNAPSHOT_SIDE as usize / 2 * gpu_snapshot::SNAPSHOT_SIDE as usize
        + gpu_snapshot::SNAPSHOT_SIDE as usize / 2)
        * 4;
    for channel in 0..3 {
        let background = u32::from(clear[pixel + channel]);
        let expected = (background * u32::from(u8::MAX - plate_alpha) + u32::from(u8::MAX) / 2)
            / u32::from(u8::MAX);
        assert!(
            (i32::from(plate[pixel + channel]) - expected as i32).abs() <= 1,
            "black plate channel {channel}: background={background}, expected={expected}, actual={}",
            plate[pixel + channel],
        );
    }
}

#[test]
fn always_passing_depth_materials_ignore_occluders_and_draw_after_opaque_geometry() {
    for depth_always in [false, true] {
        let material = TestMaterial {
            alpha_test: true,
            depth_always,
            ..Default::default()
        }
        .material(EntityRenderMaterial::Default);
        let mut descriptor = actor_pipeline_descriptor(actor_bind_group_layout());
        ActorPipelineSpecializer
            .specialize(
                ActorPipelineKey {
                    msaa: Msaa::Off,
                    hdr: false,
                    enhanced: false,
                    material: material.gpu_word(),
                    raster: material.raster(),
                },
                &mut descriptor,
            )
            .unwrap();
        // Sorted-pass spans render into the transparent pass's gamma-encoded target.
        let fragment = descriptor.fragment.as_ref().unwrap();
        let gamma = fragment.shader_defs.iter().any(
            |define| matches!(define, ShaderDefVal::Bool(name, true) if name == "ACTOR_GAMMA_BLEND"),
        );
        let srgb = fragment.targets[0].as_ref().unwrap().format.is_srgb();
        assert_eq!((gamma, srgb), (depth_always, !depth_always));
        let depth = descriptor.depth_stencil.unwrap();
        assert_eq!(depth.depth_write_enabled, Some(true));
        assert_eq!(
            depth.depth_compare == Some(bevy::render::render_resource::CompareFunction::Always),
            depth_always
        );
        assert_eq!(
            super::super::phase::sorted(material.gpu_word(), &material.raster()),
            depth_always
        );
    }
}

#[test]
fn explicit_always_depth_overrides_the_dissolve_color_default() {
    let always = TestMaterial {
        depth_always: true,
        ..Default::default()
    };
    let mut descriptor = actor_pipeline_descriptor(actor_bind_group_layout());
    ActorPipelineSpecializer
        .specialize(
            ActorPipelineKey {
                msaa: Msaa::Off,
                hdr: false,
                enhanced: false,
                material: always.word(EntityRenderMaterial::DissolveColor),
                raster: always.raster(),
            },
            &mut descriptor,
        )
        .unwrap();
    assert_eq!(
        descriptor.depth_stencil.unwrap().depth_compare,
        Some(bevy::render::render_resource::CompareFunction::Always)
    );
}

#[test]
fn ordinary_dissolve_color_finishes_before_depth_writing_transparency() {
    let material = EntityRenderMaterial::DissolveColor as u32;
    let mut descriptor = actor_pipeline_descriptor(actor_bind_group_layout());
    ActorPipelineSpecializer
        .specialize(
            ActorPipelineKey {
                msaa: Msaa::Off,
                hdr: false,
                enhanced: false,
                material,
                raster: assets::entity::material::UNAUTHORED_RENDER_STATE,
            },
            &mut descriptor,
        )
        .unwrap();
    assert!(
        !super::super::phase::sorted(material, &assets::entity::material::UNAUTHORED_RENDER_STATE),
        "an ordinary mask and color finish together before transparent terrain"
    );
    assert!(
        descriptor.fragment.unwrap().targets[0]
            .as_ref()
            .unwrap()
            .format
            .is_srgb()
    );
}

#[test]
fn legacy_leather_preserves_alpha_and_uses_mask_alpha_only_for_dye() {
    use bevy::render::render_resource::ColorWrites;
    for profile in [
        ::render_api::CombatVisuals::Modern,
        ::render_api::CombatVisuals::Legacy112,
    ] {
        for mask in [false, true] {
            let material = render_model::ActorMaterial {
                color_mask: mask,
                glint: render_model::ActorGlint {
                    profile,
                    ..Default::default()
                },
                ..TestMaterial {
                    alpha_test: true,
                    cull: false,
                    ..Default::default()
                }
                .material(EntityRenderMaterial::Default)
            };
            let mut descriptor = actor_pipeline_descriptor(actor_bind_group_layout());
            ActorPipelineSpecializer
                .specialize(
                    ActorPipelineKey {
                        msaa: Msaa::Sample4,
                        hdr: false,
                        enhanced: false,
                        material: material.gpu_word(),
                        raster: material.raster(),
                    },
                    &mut descriptor,
                )
                .unwrap();
            let legacy_mask = mask && profile == ::render_api::CombatVisuals::Legacy112;
            assert_eq!(
                descriptor.fragment.unwrap().targets[0]
                    .as_ref()
                    .unwrap()
                    .write_mask,
                if legacy_mask {
                    ColorWrites::COLOR
                } else {
                    ColorWrites::ALL
                }
            );
            assert_eq!(descriptor.multisample.alpha_to_coverage_enabled, !mask);
        }
    }
}

#[test]
fn invisible_emissive_material_keeps_eye_texels_and_rejects_alpha_endpoints() {
    let Some(gpu) = gpu_snapshot::Gpu::for_fixture(
        "invisible_emissive_material_keeps_eye_texels_and_rejects_alpha_endpoints",
    ) else {
        return;
    };
    let plane = actor_raster::cube([16, 0, 16], true, false);
    let mut material = TestMaterial {
        alpha_test: true,
        cull: false,
        emissive: true,
        only_emissive: true,
        ..Default::default()
    }
    .material(EntityRenderMaterial::Default);
    let clear = actor_raster::raster_material_target(
        &gpu,
        &plane,
        material,
        false,
        [[0; 4]; 2],
        true,
        false,
    );
    for alpha in [0, 255] {
        let frame = actor_raster::raster_material_target(
            &gpu,
            &plane,
            material,
            false,
            [[200, 120, 40, alpha]; 2],
            true,
            false,
        );
        assert_eq!(center(&frame), center(&clear), "alpha {alpha}");
    }
    let eyes = actor_raster::raster_material_target(
        &gpu,
        &plane,
        material,
        false,
        [[200, 120, 40, 128]; 2],
        true,
        false,
    );
    assert_ne!(center(&eyes), center(&clear));
    material.state.as_mut().unwrap().only_emissive = false;
    let ordinary = actor_raster::raster_material_target(
        &gpu,
        &plane,
        material,
        false,
        [[200, 120, 40, 128]; 2],
        true,
        false,
    );
    assert_eq!(center(&eyes), center(&ordinary));
}

#[test]
fn authored_depth_equal_and_channel_writes_reach_the_pipeline() {
    use bevy::render::render_resource::{ColorWrites, CompareFunction};
    for (disable_alpha_write, disable_color_write, expected) in [
        (false, false, ColorWrites::ALL),
        (true, false, ColorWrites::COLOR),
        (false, true, ColorWrites::empty()),
        (true, true, ColorWrites::empty()),
    ] {
        let state = TestMaterial {
            depth_equal: true,
            disable_alpha_write,
            disable_color_write,
            ..Default::default()
        };
        let mut descriptor = actor_pipeline_descriptor(actor_bind_group_layout());
        ActorPipelineSpecializer
            .specialize(
                ActorPipelineKey {
                    msaa: Msaa::Off,
                    hdr: false,
                    enhanced: false,
                    material: state.word(EntityRenderMaterial::Default),
                    raster: state.raster(),
                },
                &mut descriptor,
            )
            .unwrap();
        assert_eq!(
            descriptor.depth_stencil.unwrap().depth_compare,
            Some(CompareFunction::Equal)
        );
        assert_eq!(
            descriptor.fragment.unwrap().targets[0]
                .as_ref()
                .unwrap()
                .write_mask,
            expected
        );
    }
}

#[test]
fn a_world_material_replaces_the_bedrock_colour_before_host_lighting() {
    let Some(gpu) = gpu_snapshot::Gpu::for_fixture(
        "a_world_material_replaces_the_bedrock_colour_before_host_lighting",
    ) else {
        return;
    };
    let plane = actor_raster::cube([16, 0, 16], true, false);
    let material = render_model::ActorMaterial::default();
    let red = mod_render::world_material::compose(
        "fn shade(input: WorldMaterialInput) -> Surface { return Surface(vec3<f32>(param(input, 0u), 0.0, 0.0), 1.0, vec3<f32>(0.0)); }",
        0,
    )
    .unwrap();
    let frame = mod_render::world_material::frame_uniform(0.0, 1, &[1.0]);
    let defs = ["WORLD_MATERIAL", "WORLD_MATERIAL_OPAQUE"];
    let world =
        actor_raster::raster_world_material(&gpu, &plane, material, &red.library(2), &frame, &defs);
    let pixel = center(&world);
    assert!(
        pixel[0] > 200 && pixel[1] < 30 && pixel[2] < 30,
        "{pixel:?}"
    );
    // The plain surface draws the Bedrock colour unchanged.
    let plain = mod_render::world_material::compose(
        "fn shade(input: WorldMaterialInput) -> Surface { return surface(input); }",
        0,
    )
    .unwrap();
    let bedrock = actor_raster::raster_world_material(
        &gpu,
        &plane,
        material,
        &plain.library(2),
        &frame,
        &defs,
    );
    let pixel = center(&bedrock);
    assert!(
        pixel[0] > 100 && (i32::from(pixel[0]) - i32::from(pixel[1])).abs() < 4,
        "{pixel:?}"
    );
}

/// The acceptance view of a bound entity: a cutout class drops what the guest leaves uncovered,
/// and emission brightens it beyond the host's lighting.
#[test]
fn bound_entity_acceptance_follows_coverage_and_emission() {
    let Some(gpu) = gpu_snapshot::Gpu::for_fixture("bound_entity_acceptance") else {
        return;
    };
    let plane = actor_raster::cube([16, 0, 16], true, false);
    let draw = |shade: &str, class: &str| {
        let material = mod_render::world_material::compose(shade, 0).unwrap();
        let frame = mod_render::world_material::frame_uniform(0.0, 1, &[]);
        let pixels = actor_raster::raster_world_material(
            &gpu,
            &plane,
            render_model::ActorMaterial::default(),
            &material.library(2),
            &frame,
            &["WORLD_MATERIAL", class],
        );
        center(&pixels).to_vec()
    };
    let background = {
        let material = mod_render::world_material::compose(
            "fn shade(input: WorldMaterialInput) -> Surface { return Surface(vec3<f32>(1.0), 0.0, vec3<f32>(0.0)); }",
            0,
        )
        .unwrap();
        let frame = mod_render::world_material::frame_uniform(0.0, 1, &[]);
        actor_raster::raster_world_material(
            &gpu,
            &plane,
            render_model::ActorMaterial::default(),
            &material.library(2),
            &frame,
            &["WORLD_MATERIAL", "WORLD_MATERIAL_CUTOUT"],
        )
    };
    assert_eq!(
        center(&background),
        &background[..4],
        "a cutout class discards uncovered pixels"
    );
    let dark = draw(
        "fn shade(input: WorldMaterialInput) -> Surface { return Surface(vec3<f32>(0.2), 1.0, vec3<f32>(0.0)); }",
        "WORLD_MATERIAL_OPAQUE",
    );
    let glowing = draw(
        "fn shade(input: WorldMaterialInput) -> Surface { return Surface(vec3<f32>(0.2), 1.0, vec3<f32>(0.6)); }",
        "WORLD_MATERIAL_OPAQUE",
    );
    assert!(
        glowing[0] > dark[0],
        "emission adds after lighting: {dark:?} {glowing:?}"
    );
}

/// A displacing world material moves the drawn actor within its declared bound: the host
/// clamps what `displace` returns on each axis.
#[test]
fn a_displacing_world_material_moves_the_actor_within_its_bound() {
    let Some(gpu) = gpu_snapshot::Gpu::for_fixture("displacing_world_material") else {
        return;
    };
    // A one-block plane filling the frame.
    let plane = actor_raster::cube([16, 0, 16], true, false);
    let features = mod_render::world_material::Features {
        displace: true,
        ..Default::default()
    };
    let draw = |offset: &str, limit: f32| {
        let material = mod_render::world_material::compose_with(
            &format!(
                "fn shade(input: WorldMaterialInput) -> Surface {{ return Surface(vec3<f32>(1.0, 0.0, 0.0), 1.0, vec3<f32>(0.0)); }}
fn displace(input: WorldVertexInput) -> vec3<f32> {{ return vec3<f32>({offset}, 0.0, 0.0); }}"
            ),
            0,
            features,
        )
        .unwrap();
        let frame = mod_render::world_material::frame_uniform_with_effects(
            0.0,
            1,
            [[limit, 0.0, 0.0, 0.0], [0.0; 4]],
            &[],
        );
        actor_raster::raster_world_material(
            &gpu,
            &plane,
            render_model::ActorMaterial::default(),
            &material.library(2),
            &frame,
            &[
                "WORLD_MATERIAL",
                "WORLD_MATERIAL_OPAQUE",
                "WORLD_MATERIAL_DISPLACE",
            ],
        )
    };
    let red = |frame: &[u8], u: f32| {
        let side = gpu_snapshot::SNAPSHOT_SIDE as usize;
        let x = (u * side as f32) as usize;
        let pixel = &frame[(side / 2 * side + x) * 4..][..4];
        pixel[0] > 200 && pixel[1] < 30
    };
    let still = draw("0.0", 2.0);
    assert!(red(&still, 0.05) && red(&still, 0.95));
    // A quarter block to the right uncovers the frame's left eighth.
    let moved = draw("0.25", 2.0);
    assert!(!red(&moved, 0.05) && red(&moved, 0.3) && red(&moved, 0.95));
    // Ten blocks clamp to the declared quarter block.
    assert_eq!(draw("10.0", 0.25), moved);
    // A NaN offset draws in place.
    assert_eq!(draw("sqrt(-input.phase - 1.0)", 2.0), still);
}
