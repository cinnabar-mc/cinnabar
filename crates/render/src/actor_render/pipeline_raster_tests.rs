use bevy::{
    prelude::Msaa,
    render::render_resource::{
        BlendFactor, BlendOperation, ColorWrites, CompareFunction, Face, RenderPipelineDescriptor,
        Specializer,
    },
};
use render_api::render_state as api;

use super::super::{
    ActorPipelineKey, ActorPipelineSpecializer, actor_bind_group_layout, actor_pipeline_descriptor,
};

/// Specializes the ordinary actor pipeline with an authored raster state.
fn specialize(msaa: Msaa, raster: api::RenderState) -> RenderPipelineDescriptor {
    let mut descriptor = actor_pipeline_descriptor(actor_bind_group_layout());
    ActorPipelineSpecializer
        .specialize(
            ActorPipelineKey {
                msaa,
                hdr: false,
                enhanced: false,
                material: assets::EntityRenderMaterial::Default as u32,
                raster: Some(raster),
            },
            &mut descriptor,
        )
        .unwrap();
    descriptor
}

#[test]
fn every_legacy_word_specializes_like_its_projected_raster() {
    for bits in 0..512u32 {
        let state = assets::EntityRenderMaterialState {
            alpha_test: bits & 1 != 0,
            cull: bits & 2 != 0,
            blend: bits & 4 != 0,
            depth_write: bits & 8 != 0,
            emissive: bits & 16 != 0,
            additive: bits & 32 != 0,
            additive_alpha: bits & 64 != 0,
            disable_overlay: bits & 128 != 0,
            depth_always: bits & 256 != 0,
        };
        let material = assets::EntityRenderMaterial::Default.word(Some(state));
        for msaa in [Msaa::Off, Msaa::Sample4] {
            let legacy = ActorPipelineKey {
                msaa,
                hdr: false,
                enhanced: false,
                material,
                raster: None,
            };
            let authored = ActorPipelineKey {
                raster: Some(state.render_state()),
                ..legacy
            };
            assert!(legacy.contract() == authored.contract(), "{state:?}");
        }
    }
}

#[test]
fn inverted_culling_discards_front_faces() {
    let descriptor = specialize(
        Msaa::Off,
        api::RenderState {
            cull: api::CullMode::Front,
            ..Default::default()
        },
    );
    assert_eq!(descriptor.primitive.cull_mode, Some(Face::Front));
}

#[test]
fn authored_forward_comparisons_flip_for_reversed_depth() {
    for (authored, reversed) in [
        (api::CompareFunction::Less, CompareFunction::Greater),
        (
            api::CompareFunction::LessEqual,
            CompareFunction::GreaterEqual,
        ),
        (api::CompareFunction::Greater, CompareFunction::Less),
        (api::CompareFunction::Equal, CompareFunction::Equal),
    ] {
        let descriptor = specialize(
            Msaa::Off,
            api::RenderState {
                depth: api::DepthState {
                    compare: Some(authored),
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        let depth = descriptor.depth_stencil.unwrap();
        assert_eq!(depth.depth_compare, Some(reversed), "{authored:?}");
        assert_eq!(depth.depth_write_enabled, Some(true));
    }
}

#[test]
fn a_disabled_depth_test_neither_tests_nor_writes() {
    let descriptor = specialize(
        Msaa::Off,
        api::RenderState {
            depth: api::DepthState {
                compare: None,
                write: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let depth = descriptor.depth_stencil.unwrap();
    assert_eq!(depth.depth_compare, Some(CompareFunction::Always));
    assert_eq!(depth.depth_write_enabled, Some(false));
}

#[test]
fn colour_and_alpha_blend_independently() {
    let descriptor = specialize(
        Msaa::Off,
        api::RenderState {
            blend: Some(api::BlendState {
                color: api::BlendComponent {
                    source: api::BlendFactor::DestinationColor,
                    destination: api::BlendFactor::Zero,
                    operation: api::BlendOperation::Add,
                },
                alpha: api::BlendComponent {
                    source: api::BlendFactor::One,
                    destination: api::BlendFactor::OneMinusSourceAlpha,
                    operation: api::BlendOperation::ReverseSubtract,
                },
            }),
            write_alpha: false,
            ..Default::default()
        },
    );
    let target = descriptor.fragment.unwrap().targets[0].clone().unwrap();
    let blend = target.blend.unwrap();
    assert_eq!(
        (blend.color.src_factor, blend.color.dst_factor),
        (BlendFactor::Dst, BlendFactor::Zero)
    );
    assert_eq!(
        (
            blend.alpha.src_factor,
            blend.alpha.dst_factor,
            blend.alpha.operation
        ),
        (
            BlendFactor::One,
            BlendFactor::OneMinusSrcAlpha,
            BlendOperation::ReverseSubtract
        )
    );
    assert_eq!(target.write_mask, ColorWrites::COLOR);
}

#[test]
fn authored_alpha_to_coverage_needs_samples_and_no_blending() {
    let coverage = api::RenderState {
        alpha_to_coverage: true,
        ..Default::default()
    };
    let enabled = |msaa, raster| {
        specialize(msaa, raster)
            .multisample
            .alpha_to_coverage_enabled
    };
    assert!(enabled(Msaa::Sample4, coverage));
    assert!(!enabled(Msaa::Off, coverage));
    assert!(!enabled(
        Msaa::Sample4,
        api::RenderState {
            blend: Some(api::BlendState::ALPHA),
            ..coverage
        }
    ));
}

#[test]
fn states_differing_only_in_unlowered_stencil_or_bias_share_a_contract() {
    let key = |raster| ActorPipelineKey {
        msaa: Msaa::Off,
        hdr: false,
        enhanced: false,
        material: assets::EntityRenderMaterial::Default as u32,
        raster: Some(raster),
    };
    let plain = api::RenderState::default();
    let authored = api::RenderState {
        stencil: Some(api::StencilState {
            reference: 3,
            ..Default::default()
        }),
        depth: api::DepthState {
            bias: api::DepthBias {
                constant: 2.0,
                slope_scale: 1.0,
            },
            ..Default::default()
        },
        ..plain
    };
    assert!(key(plain).contract() == key(authored).contract());
}
