//! The actor shading features and layer fields a resolved material's defines select.
//!
//! The define-to-texture and define-to-uniform wiring follows Bedrock's actor frame
//! extraction; the shader formulas are unknown, so the actor shader's handling of
//! the newer flags is provisional.

use assets::{EntityRenderMaterialState, entity::material::EntityMaterialState};

use super::{Resolved, actor_type};

pub(super) const OVERLAY: &str = "USE_OVERLAY";
pub(super) const ALPHA_TEST: &str = "ALPHA_TEST";
pub(super) const EMISSIVE: &str = "USE_EMISSIVE";
pub(super) const ONLY_EMISSIVE: &str = "USE_ONLY_EMISSIVE";
pub(super) const COLOR_MASK: &str = "USE_COLOR_MASK";
pub(super) const MULTITEXTURE: &str = "USE_MULTITEXTURE";
pub(super) const GLINT: &str = "GLINT";
pub(super) const MULTIPLICATIVE_TINT: &str = "MULTIPLICATIVE_TINT";
pub(super) const MASKED_MULTITEXTURE: &str = "MASKED_MULTITEXTURE";
pub(super) const TINTED_ALPHA_TEST: &str = "TINTED_ALPHA_TEST";
pub(super) const COLOR_SECOND_TEXTURE: &str = "COLOR_SECOND_TEXTURE";
pub(super) const UV_ANIM: &str = "USE_UV_ANIM";
/// The actor defines the client draws: those [`shading`] reads, and `USE_UV_ANIM`, which
/// [`applies_uv_anim`] reads. Every other actor define is carried without being drawn.
pub(super) const APPLIED_DEFINES: [&str; 12] = [
    ALPHA_TEST,
    EMISSIVE,
    ONLY_EMISSIVE,
    OVERLAY,
    COLOR_MASK,
    MULTITEXTURE,
    GLINT,
    MULTIPLICATIVE_TINT,
    MASKED_MULTITEXTURE,
    TINTED_ALPHA_TEST,
    COLOR_SECOND_TEXTURE,
    UV_ANIM,
];

/// The shading features an actor draw reads: [`shading`] for the material's actor material
/// type. A material whose program has no actor type draws as plain `Actor`, whose name the
/// multitexture define no longer selects.
pub(super) fn actor_shading(resolved: &Resolved) -> EntityRenderMaterialState {
    let mut state = shading(resolved);
    if actor_type::of(resolved) == actor_type::ActorMaterialType::Other {
        // Plain `Actor` has neither the glint, multitexture or tint material names nor the
        // masked-multitexture and second-texture variant flags.
        state.multitexture = false;
        state.glint = false;
        state.texture_tint = false;
        state.texture_mask = false;
        state.color_second_texture = false;
    }
    state
}

/// Whether an actor draw of `resolved` applies its controller's `uv_anim`: Bedrock binds the
/// `UVAnimation` uniform only with `USE_UV_ANIM`. A material inheriting a native base the
/// client declares itself keeps applying it, because those stand-ins do not carry every define
/// the shipped declaration has.
pub(super) fn applies_uv_anim(resolved: &Resolved) -> bool {
    resolved.defines.contains(UV_ANIM) || resolved.assumes_entity_program
}

/// The shading features the actor shader reads from a definition's defines.
pub(super) fn shading(resolved: &Resolved) -> EntityRenderMaterialState {
    EntityRenderMaterialState {
        // Native JSON actors select the transparent pass before legacy ALPHA_TEST; that pass
        // preserves sampled opacity without a cutout discard.
        // `TINTED_ALPHA_TEST` joins `ALPHA_TEST` in Bedrock's pass choice.
        alpha_test: (resolved.defines.contains(ALPHA_TEST)
            || resolved.defines.contains(TINTED_ALPHA_TEST))
            && !resolved.states.contains(&EntityMaterialState::Blending),
        emissive: resolved.defines.contains(EMISSIVE),
        only_emissive: resolved.defines.contains(ONLY_EMISSIVE),
        // Bedrock binds the overlay colour with `USE_OVERLAY` or `TINTED_ALPHA_TEST`.
        disable_overlay: !resolved.defines.contains(OVERLAY)
            && !resolved.defines.contains(TINTED_ALPHA_TEST),
        color_mask: resolved.defines.contains(COLOR_MASK),
        multitexture: resolved.defines.contains(MULTITEXTURE),
        glint: resolved.defines.contains(GLINT),
        texture_tint: resolved.defines.contains(MULTIPLICATIVE_TINT),
        texture_mask: resolved.defines.contains(MASKED_MULTITEXTURE),
        color_second_texture: resolved.defines.contains(COLOR_SECOND_TEXTURE)
            && resolved.defines.contains(MULTITEXTURE),
    }
}
