//! Instance-word variations presentation applies on top of a layer's authored material, so the
//! renderer can compile every pipeline a native material can reach before the first draw.

use super::frame::ActorMaterial;

impl ActorMaterial {
    /// This material as a worn layer draws it under the 1.12 combat profile: the alpha-tested
    /// entity material with the unauthored raster, or the glint kind for enchanted items.
    #[must_use]
    pub fn legacy_worn(self, glint: bool) -> Self {
        Self {
            kind: if glint {
                assets::EntityRenderMaterial::Glint
            } else {
                assets::EntityRenderMaterial::Default
            },
            state: Some(assets::EntityRenderMaterialState {
                alpha_test: true,
                ..Default::default()
            }),
            render_state: Some(assets::entity::material::UNAUTHORED_RENDER_STATE),
            ..self
        }
    }

    /// The (instance word, raster) pair of each of [`Self::presentation_variants`], which is
    /// what the renderer compiles a pipeline for.
    pub fn presentation_rasters(
        self,
    ) -> impl Iterator<Item = (u32, render_api::render_state::RenderState)> {
        self.presentation_variants()
            .map(|material| (material.gpu_word(), material.raster()))
    }

    /// Every material a layer authored as `self` can draw with once presentation applies its
    /// draw-time choices: both combat profiles, the glint kind of enchanted worn and held
    /// layers, the dye mask, overlay suppression and the 1.12 worn material.
    pub fn presentation_variants(self) -> impl Iterator<Item = Self> {
        // Naming every field makes a new one fail to compile here until its variations are
        // either enumerated below or shown not to reach the instance word.
        let Self {
            color_mask: _,
            glint:
                super::ActorGlint {
                    profile: _,
                    time_seconds: _,
                    strength: _,
                    speed: _,
                    uv_scale: _,
                    uv_extent: _,
                    texture: _,
                },
            kind,
            state: _,
            render_state: _,
            suppress_overlay: _,
            dissolve_multiplier: _,
            light_color_multiplier: _,
            // Masks are instance data; they reach neither the word nor the raster.
            masks: _,
        } = self;
        [kind, assets::EntityRenderMaterial::Glint]
            .into_iter()
            .flat_map(move |kind| {
                [
                    render_api::CombatVisuals::Modern,
                    render_api::CombatVisuals::Legacy112,
                ]
                .into_iter()
                .flat_map(move |profile| {
                    [false, true].into_iter().flat_map(move |color_mask| {
                        [false, true].map(move |suppress_overlay| Self {
                            kind,
                            color_mask,
                            suppress_overlay,
                            glint: super::ActorGlint {
                                profile,
                                ..self.glint
                            },
                            ..self
                        })
                    })
                })
            })
            .flat_map(|material| {
                let worn =
                    (material.glint.profile == render_api::CombatVisuals::Legacy112).then(|| {
                        material.legacy_worn(material.kind == assets::EntityRenderMaterial::Glint)
                    });
                std::iter::once(material).chain(worn)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variants_reach_every_presentation_flag_of_the_instance_word() {
        let base = ActorMaterial::default();
        let words = base
            .presentation_variants()
            .map(ActorMaterial::gpu_word)
            .fold(0, |all, word| all | word);
        for flag in [
            super::super::glint::ACTOR_COLOR_MASK_MATERIAL_FLAG,
            super::super::glint::LEGACY_COMBAT_MATERIAL_FLAG,
            assets::EntityRenderMaterialState::DISABLE_OVERLAY,
            assets::EntityRenderMaterial::Glint as u32,
        ] {
            assert_eq!(words & flag, flag, "{flag:#x}");
        }
        let legacy = ActorMaterial {
            glint: super::super::ActorGlint {
                profile: render_api::CombatVisuals::Legacy112,
                ..base.glint
            },
            ..base
        };
        for glint in [false, true] {
            assert!(
                base.presentation_variants()
                    .any(|material| material == legacy.legacy_worn(glint)),
                "the 1.12 worn material is reachable: {glint}"
            );
        }
    }
}
