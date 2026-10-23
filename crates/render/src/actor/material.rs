use assets::EntityRenderMaterialState;

#[cfg(test)]
use render_model::ActorMaterial;

/// Renderer-only routing for a color layer that must follow a sorted dissolve mask.
pub(crate) const LATE_DISSOLVE_COLOR: u32 = 1 << 31;

/// Translates admitted actor blending into the GPU pipeline state.
pub fn blend_state(
    state: EntityRenderMaterialState,
) -> Option<bevy::render::render_resource::BlendState> {
    use bevy::render::render_resource::{BlendComponent, BlendFactor, BlendOperation, BlendState};
    if !state.blend {
        return None;
    }
    let component = BlendComponent {
        src_factor: if state.additive && !state.additive_alpha {
            BlendFactor::One
        } else {
            BlendFactor::SrcAlpha
        },
        dst_factor: if state.additive {
            BlendFactor::One
        } else {
            BlendFactor::OneMinusSrcAlpha
        },
        operation: BlendOperation::Add,
    };
    Some(BlendState {
        color: component,
        alpha: component,
    })
}

pub(crate) fn state(word: u32) -> Option<EntityRenderMaterialState> {
    EntityRenderMaterialState::from_word(word & !LATE_DISSOLVE_COLOR)
}

/// The raster state an instance word selects when its frame supplies none.
pub(crate) fn word_raster(word: u32) -> render_api::render_state::RenderState {
    state(word).map_or(
        EntityRenderMaterialState::UNAUTHORED,
        EntityRenderMaterialState::render_state,
    )
}

/// An instance's raster state from its frame table, or its word's projection without one.
pub(crate) fn instance_raster(
    rasters: &[render_api::render_state::RenderState],
    instance: &render_model::ActorGpuInstance,
) -> render_api::render_state::RenderState {
    rasters
        .get(instance.raster as usize)
        .copied()
        .unwrap_or_else(|| word_raster(instance.material))
}

/// The part of a raster state actor pipelines apply. Stencil and depth bias stay authored but are
/// not lowered, so states differing only there share one pipeline.
pub(crate) fn lowered(
    raster: render_api::render_state::RenderState,
) -> render_api::render_state::RenderState {
    render_api::render_state::RenderState {
        stencil: None,
        depth: render_api::render_state::DepthState {
            bias: Default::default(),
            ..raster.depth
        },
        ..raster
    }
}

/// Whether draws with this raster state belong to the sorted translucent pass: they blend or
/// ignore stored depth.
pub(crate) fn raster_sorted(raster: &render_api::render_state::RenderState) -> bool {
    use render_api::render_state::CompareFunction;
    raster.blend.is_some()
        || !matches!(raster.depth.compare, Some(compare) if compare != CompareFunction::Always)
}

/// Applies a raster state to an actor pipeline. Comparisons are authored for forward depth and
/// flip for the scene's reversed depth buffer. Depth bias and stencil are not lowered: the scene
/// depth format has no stencil aspect, and the client's bias scale is unresolved.
pub(crate) fn lower_raster(
    descriptor: &mut bevy::render::render_resource::RenderPipelineDescriptor,
    raster: &render_api::render_state::RenderState,
) {
    use bevy::render::render_resource::{
        BlendComponent, BlendFactor, BlendOperation, BlendState, ColorWrites, CompareFunction, Face,
    };
    use render_api::render_state as api;
    descriptor.primitive.cull_mode = match raster.cull {
        api::CullMode::None => None,
        api::CullMode::Front => Some(Face::Front),
        api::CullMode::Back => Some(Face::Back),
    };
    let depth = descriptor
        .depth_stencil
        .as_mut()
        .expect("actor pipelines test depth");
    depth.depth_compare = Some(match raster.depth.compare {
        None | Some(api::CompareFunction::Always) => CompareFunction::Always,
        Some(api::CompareFunction::Never) => CompareFunction::Never,
        Some(api::CompareFunction::Less) => CompareFunction::Greater,
        Some(api::CompareFunction::LessEqual) => CompareFunction::GreaterEqual,
        Some(api::CompareFunction::Greater) => CompareFunction::Less,
        Some(api::CompareFunction::GreaterEqual) => CompareFunction::LessEqual,
        Some(api::CompareFunction::Equal) => CompareFunction::Equal,
        Some(api::CompareFunction::NotEqual) => CompareFunction::NotEqual,
    });
    // A disabled depth test also disables depth writes.
    depth.depth_write_enabled = Some(raster.depth.write && raster.depth.compare.is_some());
    let factor = |factor| match factor {
        api::BlendFactor::Zero => BlendFactor::Zero,
        api::BlendFactor::One => BlendFactor::One,
        api::BlendFactor::SourceColor => BlendFactor::Src,
        api::BlendFactor::OneMinusSourceColor => BlendFactor::OneMinusSrc,
        api::BlendFactor::SourceAlpha => BlendFactor::SrcAlpha,
        api::BlendFactor::OneMinusSourceAlpha => BlendFactor::OneMinusSrcAlpha,
        api::BlendFactor::DestinationColor => BlendFactor::Dst,
        api::BlendFactor::OneMinusDestinationColor => BlendFactor::OneMinusDst,
        api::BlendFactor::DestinationAlpha => BlendFactor::DstAlpha,
        api::BlendFactor::OneMinusDestinationAlpha => BlendFactor::OneMinusDstAlpha,
    };
    let component = |component: api::BlendComponent| BlendComponent {
        src_factor: factor(component.source),
        dst_factor: factor(component.destination),
        operation: match component.operation {
            api::BlendOperation::Add => BlendOperation::Add,
            api::BlendOperation::Subtract => BlendOperation::Subtract,
            api::BlendOperation::ReverseSubtract => BlendOperation::ReverseSubtract,
            api::BlendOperation::Min => BlendOperation::Min,
            api::BlendOperation::Max => BlendOperation::Max,
        },
    };
    let target = descriptor
        .fragment
        .as_mut()
        .and_then(|fragment| fragment.targets.first_mut())
        .and_then(Option::as_mut)
        .expect("actor pipelines draw one color target");
    target.blend = raster.blend.map(|blend| BlendState {
        color: component(blend.color),
        alpha: component(blend.alpha),
    });
    target.write_mask = match (raster.write_color, raster.write_alpha) {
        (true, true) => ColorWrites::ALL,
        (true, false) => ColorWrites::COLOR,
        (false, true) => ColorWrites::ALPHA,
        (false, false) => ColorWrites::empty(),
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authored_actor_states_roundtrip_without_changing_the_shader_kind() {
        for bits in 0..8192 {
            let expected = EntityRenderMaterialState {
                alpha_test: bits & 1 != 0,
                cull: bits & 2 != 0,
                blend: bits & 4 != 0,
                depth_write: bits & 8 != 0,
                emissive: bits & 16 != 0,
                additive: bits & 32 != 0,
                additive_alpha: bits & 64 != 0,
                disable_overlay: bits & 128 != 0,
                depth_always: bits & 256 != 0,
                only_emissive: bits & 512 != 0,
                depth_equal: bits & 1024 != 0,
                disable_alpha_write: bits & 2048 != 0,
                disable_color_write: bits & 4096 != 0,
            };
            let material = ActorMaterial {
                kind: assets::EntityRenderMaterial::Default,
                state: Some(expected),
                ..Default::default()
            };
            let word = material.gpu_word();
            assert_eq!(state(word), Some(expected));
            assert_eq!(
                word & EntityRenderMaterialState::KIND_MASK,
                material.kind as u32
            );
        }
        assert_eq!(
            ActorMaterial {
                glint: render_model::ActorGlint {
                    profile: render_api::CombatVisuals::Modern,
                    ..Default::default()
                },
                ..Default::default()
            }
            .gpu_word(),
            assets::EntityRenderMaterial::Default as u32
        );
        assert_eq!(state(ActorMaterial::default().gpu_word()), None);
    }
}
