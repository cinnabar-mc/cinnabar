//! Test vocabulary for the flag combinations built-in actor materials draw with.

use assets::entity::material::EntityMaterialDefinition;
use assets::{EntityRenderMaterial, EntityRenderMaterialState};
use render_api::render_state::RenderState;

/// One built-in actor material as shading features plus the raster flags it implies. The
/// default culls back faces and writes depth, like `entity`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TestMaterial {
    pub alpha_test: bool,
    pub cull: bool,
    pub blend: bool,
    pub depth_write: bool,
    pub emissive: bool,
    pub only_emissive: bool,
    /// With `blend`, adds to the destination.
    pub additive: bool,
    /// With `additive`, weights the source by its alpha.
    pub additive_alpha: bool,
    pub disable_overlay: bool,
    pub depth_always: bool,
    pub depth_equal: bool,
    pub disable_alpha_write: bool,
    pub disable_color_write: bool,
}

impl Default for TestMaterial {
    fn default() -> Self {
        Self {
            alpha_test: false,
            cull: true,
            blend: false,
            depth_write: true,
            emissive: false,
            only_emissive: false,
            additive: false,
            additive_alpha: false,
            disable_overlay: false,
            depth_always: false,
            depth_equal: false,
            disable_alpha_write: false,
            disable_color_write: false,
        }
    }
}

impl TestMaterial {
    /// The shading features the instance word carries.
    pub(crate) fn shading(self) -> EntityRenderMaterialState {
        EntityRenderMaterialState {
            alpha_test: self.alpha_test,
            emissive: self.emissive,
            only_emissive: self.only_emissive,
            disable_overlay: self.disable_overlay,
        }
    }

    /// The resolved entity material these flags describe, built the way the native material
    /// table resolves its declarations.
    pub(crate) fn definition(self) -> EntityMaterialDefinition {
        assets::facts::pack::NativeMaterialState {
            alpha_test: self.alpha_test,
            cull: self.cull,
            blend: self.blend,
            depth_write: self.depth_write,
            emissive: self.emissive,
            only_emissive: self.only_emissive,
            additive: self.additive,
            additive_alpha: self.additive_alpha,
            disable_overlay: self.disable_overlay,
            depth_always: self.depth_always,
            depth_equal: self.depth_equal,
            disable_alpha_write: self.disable_alpha_write,
            disable_color_write: self.disable_color_write,
        }
        .definition()
    }

    /// The raster state these flags select, lowered by the real entity material path.
    pub(crate) fn raster(self) -> RenderState {
        self.definition().render_state()
    }

    /// The renderer material for `kind` drawn with these flags.
    pub(crate) fn material(self, kind: EntityRenderMaterial) -> render_model::ActorMaterial {
        render_model::ActorMaterial {
            kind,
            state: Some(self.shading()),
            render_state: Some(self.raster()),
            ..Default::default()
        }
    }

    /// The instance word `kind` draws with under these flags.
    pub(crate) fn word(self, kind: EntityRenderMaterial) -> u32 {
        self.material(kind).gpu_word()
    }
}
