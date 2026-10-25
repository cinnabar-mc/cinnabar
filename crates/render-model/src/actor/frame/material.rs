//! The material one actor draw shades with, and its shared instance word.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActorMaterial {
    /// Alpha weights leather dye independently of the page's other materials.
    pub color_mask: bool,
    /// Animated glint factors. Item glint overlays preserve the base material's coverage.
    pub glint: crate::actor::ActorGlint,
    pub kind: assets::EntityRenderMaterial,
    /// Shading features of the authored material; `None` when the layer names none.
    pub state: Option<assets::EntityRenderMaterialState>,
    /// Raster state of the authored material; `None` draws as a layer without one.
    pub render_state: Option<render_api::render_state::RenderState>,
    /// Suppresses overlays without changing whether raster state was authored.
    pub suppress_overlay: bool,
    /// Alpha-test multiplier remains a float because authored dissolve values exceed one.
    pub dissolve_multiplier: f32,
    /// RGB illumination multiplier; applies to lit and unlit draws without clamping.
    pub light_color_multiplier: f32,
    /// Cinnabar material masks on the draw's artwork page.
    pub masks: crate::actor::ActorMaterialMasks,
    /// One more than the slot of the programmable world material that shades this draw; zero
    /// draws the Bedrock material alone.
    pub world: u8,
    /// The host's raster for [`Self::world`]'s class. It replaces [`Self::render_state`] only
    /// once the world material's pipeline is ready; until then the draw stays Bedrock's.
    pub world_raster: Option<render_api::render_state::RenderState>,
}

impl ActorMaterial {
    /// Whether color overlays may affect this material's fragment color and vertex lighting.
    /// Dissolve alpha belongs only to its coverage parameter; authored suppression also wins.
    #[must_use]
    pub fn allows_overlay(self) -> bool {
        if self.suppress_overlay {
            return false;
        }
        match self.kind {
            assets::EntityRenderMaterial::Default
            | assets::EntityRenderMaterial::Dragon
            | assets::EntityRenderMaterial::Glint => {
                self.state.is_none_or(|state| !state.disable_overlay)
            }
            assets::EntityRenderMaterial::DissolveDepth
            | assets::EntityRenderMaterial::DissolveColor => false,
        }
    }
}

impl Default for ActorMaterial {
    fn default() -> Self {
        Self {
            color_mask: false,
            glint: Default::default(),
            kind: Default::default(),
            state: None,
            render_state: None,
            suppress_overlay: false,
            dissolve_multiplier: 1.0,
            light_color_multiplier: 1.0,
            masks: crate::actor::ActorMaterialMasks::NONE,
            world: 0,
            world_raster: None,
        }
    }
}

impl ActorMaterial {
    /// The raster state this material draws with.
    #[must_use]
    pub fn raster(self) -> render_api::render_state::RenderState {
        self.render_state
            .unwrap_or(assets::entity::material::UNAUTHORED_RENDER_STATE)
    }

    /// Shader kind and shading flags in the shared instance word, as Bedrock draws this
    /// material. An authored material's cull and blend bits, which the shader reads, follow its
    /// raster state. A world material appears only in [`Self::world_draw`].
    pub fn gpu_word(self) -> u32 {
        self.word_with(self.raster(), 0)
    }

    /// The instance word and raster this material draws with once its world material's
    /// pipeline is ready: [`Self::gpu_word`] tagged with [`Self::world`] and with the cull and
    /// blend bits of the host's raster. `None` without a world material.
    #[must_use]
    pub fn world_draw(self) -> Option<(u32, render_api::render_state::RenderState)> {
        let raster = self.world_raster.filter(|_| self.world != 0)?;
        Some((self.word_with(raster, self.world), raster))
    }

    /// The instance word of this material drawn with `raster`, tagged with `world`.
    fn word_with(self, raster: render_api::render_state::RenderState, world: u8) -> u32 {
        let flag = |enabled: bool, flag: u32| {
            if enabled && self.state.is_some() {
                flag
            } else {
                0
            }
        };
        self.kind.word(self.state)
            | (u32::from(world) << super::ACTOR_WORLD_MATERIAL_SHIFT)
            | flag(
                raster.cull == render_api::render_state::CullMode::Back,
                assets::EntityRenderMaterialState::CULL,
            )
            | flag(
                raster.blend.is_some(),
                assets::EntityRenderMaterialState::BLEND,
            )
            | if self.suppress_overlay {
                assets::EntityRenderMaterialState::DISABLE_OVERLAY
            } else {
                0
            }
            | if self.color_mask {
                crate::actor::glint::ACTOR_COLOR_MASK_MATERIAL_FLAG
            } else {
                0
            }
            | if self.glint.profile == render_api::CombatVisuals::Legacy112 {
                crate::actor::glint::LEGACY_COMBAT_MATERIAL_FLAG
            } else {
                0
            }
    }
}
