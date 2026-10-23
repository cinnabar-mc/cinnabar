use assets::EntityRenderMaterialState;

#[cfg(test)]
use render_model::ActorMaterial;

/// Renderer-only routing for a color layer that must follow a sorted dissolve mask.
pub(crate) const LATE_DISSOLVE_COLOR: u32 = 1 << 31;

/// Additive blending: `source * factor + destination` for colour and alpha.
pub(crate) fn additive(
    source: render_api::render_state::BlendFactor,
) -> render_api::render_state::BlendState {
    use render_api::render_state::{BlendComponent, BlendFactor, BlendOperation, BlendState};
    let component = BlendComponent {
        source,
        destination: BlendFactor::One,
        operation: BlendOperation::Add,
    };
    BlendState {
        color: component,
        alpha: component,
    }
}

/// Shading features of an instance word, ignoring renderer-only routing bits.
pub(crate) fn state(word: u32) -> Option<EntityRenderMaterialState> {
    EntityRenderMaterialState::from_word(word & !LATE_DISSOLVE_COLOR)
}

/// An instance's raster state from its frame table; an index outside it draws unauthored.
pub(crate) fn instance_raster(
    rasters: &[render_api::render_state::RenderState],
    instance: &render_model::ActorGpuInstance,
) -> render_api::render_state::RenderState {
    rasters
        .get(instance.raster as usize)
        .copied()
        .unwrap_or(assets::entity::material::UNAUTHORED_RENDER_STATE)
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
    raster.blend.is_some() || raster.ignores_stored_depth()
}

/// The GPU blend state of a raster state; `None` writes the source unblended.
pub fn blend_state(
    raster: &render_api::render_state::RenderState,
) -> Option<bevy::render::render_resource::BlendState> {
    use bevy::render::render_resource::{BlendComponent, BlendFactor, BlendOperation, BlendState};
    use render_api::render_state as api;
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
    raster.blend.map(|blend| BlendState {
        color: component(blend.color),
        alpha: component(blend.alpha),
    })
}

/// Applies a raster state to an actor pipeline. Comparisons are authored for forward depth and
/// flip for the scene's reversed depth buffer. Depth bias and stencil are not lowered: the scene
/// depth format has no stencil aspect, and the client's bias scale is unresolved.
pub(crate) fn lower_raster(
    descriptor: &mut bevy::render::render_resource::RenderPipelineDescriptor,
    raster: &render_api::render_state::RenderState,
) {
    use bevy::render::render_resource::{ColorWrites, CompareFunction, Face};
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
    let target = descriptor
        .fragment
        .as_mut()
        .and_then(|fragment| fragment.targets.first_mut())
        .and_then(Option::as_mut)
        .expect("actor pipelines draw one color target");
    target.blend = blend_state(raster);
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
        for bits in 0..32 {
            let expected = EntityRenderMaterialState {
                alpha_test: bits & 1 != 0,
                emissive: bits & 2 != 0,
                disable_overlay: bits & 4 != 0,
                only_emissive: bits & 8 != 0,
                color_mask: bits & 16 != 0,
                multitexture: false,
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
