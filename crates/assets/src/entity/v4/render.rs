use serde::{Deserialize, Serialize};

use crate::AssetError;

use super::super::{CompiledEntityAssets, invalid};
use super::{range_in_bounds, validate_flattened_ranges};

pub const MAX_ENTITY_RENDER_LAYERS: usize = 16_384;
pub const MAX_ENTITY_RENDER_SLOTS: usize = 65_536;
pub const MAX_ENTITY_RENDER_CANDIDATES: usize = 262_144;
pub const MAX_ENTITY_RENDER_VISIBILITY: usize = 262_144;
pub const MAX_ENTITY_RENDER_PATTERN_BYTES: usize = 64;

/// Supported entity shader and depth contracts selected by authored materials.
#[derive(
    Clone, Copy, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
#[repr(u32)]
pub enum EntityRenderMaterial {
    #[default]
    Default,
    Dragon,
    DissolveDepth,
    DissolveColor,
    Glint,
}

/// Shading features of an authored entity material. Its raster state comes from the resolved
/// material definition.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityRenderMaterialState {
    pub alpha_test: bool,
    /// Texture alpha weights world lighting; colored alpha-zero texels remain emissive.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub emissive: bool,
    /// Keeps only texels whose emissive weight lies between the transparent and lit endpoints.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub only_emissive: bool,
    /// Suppresses actor overlays and their effect on vertex lighting.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub disable_overlay: bool,
    /// `USE_COLOR_MASK`: texture alpha weights the dye multiplier instead of coverage.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub color_mask: bool,
    /// `USE_MULTITEXTURE`: the draw samples its controller's first three textures together.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub multitexture: bool,
}

impl EntityRenderMaterialState {
    pub const KIND_MASK: u32 = 0xff;
    pub const AUTHORED: u32 = 1 << 8;
    pub const ALPHA_TEST: u32 = 1 << 9;
    /// Set from the raster state: back faces are culled.
    pub const CULL: u32 = 1 << 10;
    /// Set from the raster state: the draw blends.
    pub const BLEND: u32 = 1 << 11;
    pub const EMISSIVE: u32 = 1 << 12;
    pub const ONLY_EMISSIVE: u32 = 1 << 13;
    pub const DISABLE_OVERLAY: u32 = 1 << 14;
    /// `USE_COLOR_MASK`: texture alpha weights the dye multiplier.
    pub const COLOR_MASK: u32 = 1 << 15;

    /// Reads the word's shading flags; `None` when no material was authored. Multitexturing
    /// travels in the instance's texture layers instead.
    #[must_use]
    pub fn from_word(word: u32) -> Option<Self> {
        (word & Self::AUTHORED != 0).then_some(Self {
            alpha_test: word & Self::ALPHA_TEST != 0,
            emissive: word & Self::EMISSIVE != 0,
            only_emissive: word & Self::ONLY_EMISSIVE != 0,
            disable_overlay: word & Self::DISABLE_OVERLAY != 0,
            color_mask: word & Self::COLOR_MASK != 0,
            ..Self::default()
        })
    }
}

impl EntityRenderMaterial {
    /// Packs the shader kind with optional authored shading features.
    pub fn word(self, state: Option<EntityRenderMaterialState>) -> u32 {
        let kind = self as u32;
        let Some(state) = state else {
            return kind;
        };
        let flag = |enabled: bool, flag: u32| if enabled { flag } else { 0 };
        kind | EntityRenderMaterialState::AUTHORED
            | flag(state.alpha_test, EntityRenderMaterialState::ALPHA_TEST)
            | flag(state.emissive, EntityRenderMaterialState::EMISSIVE)
            | flag(
                state.only_emissive,
                EntityRenderMaterialState::ONLY_EMISSIVE,
            )
            | flag(
                state.disable_overlay,
                EntityRenderMaterialState::DISABLE_OVERLAY,
            )
            | flag(state.color_mask, EntityRenderMaterialState::COLOR_MASK)
    }
}

pub const ENTITY_ALPHA_TEST_THRESHOLD: f32 = 0.5;

/// One render controller of a rig: the expressions that pick its textures, hidden parts and
/// colours each tick. Layers of a rig are contiguous and ordered by `rig`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityRenderLayer {
    #[serde(default, skip_serializing_if = "is_zero")]
    pub material: EntityRenderMaterial,
    /// Shading features of the resolved material; absent when the layer names none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material_state: Option<EntityRenderMaterialState>,
    /// Index into [`EntityRenderData::materials`]; absent when the layer names no material.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material_definition: Option<u32>,
    /// Index into [`EntityRenderData::material_masks`] for a Cinnabar material with masks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material_masks: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hurt_color: Option<[u32; 4]>,
    /// Index into the rig bindings.
    pub rig: u32,
    /// The entity's activation expression for this controller; absent means always.
    pub condition: Option<u32>,
    /// Root of a whole-model material decision, evaluated once after controller activation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material_selection: Option<u32>,
    pub first_slot: u32,
    pub slot_count: u16,
    /// Authored texture references could not resolve; activation still controls fallback.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub unresolved_textures: bool,
    pub first_visibility: u32,
    pub visibility_count: u16,
    /// `[r, g, b, a]` expressions multiplying the texture.
    pub color: Option<[u32; 4]>,
    /// `[r, g, b, a]` expressions blended over the texture.
    pub overlay_color: Option<[u32; 4]>,
    pub on_fire_color: Option<[u32; 4]>,
    /// `uv_anim` `[offset u, offset v, scale u, scale v]` expressions; absent is identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uv_anim: Option<[u32; 4]>,
    /// The controller's `geometry` choices; none draws with the rig's own geometry.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub first_geometry: u32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub geometry_count: u16,
    /// Draws unlit, ignoring world light.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub ignore_lighting: bool,
    /// Expression multiplying light RGB, including controllers that ignore world lighting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub light_color_multiplier: Option<u32>,
}

fn is_zero<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

/// One leaf of a controller's `geometry` expression: the first whose condition holds is drawn.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityRenderGeometry {
    pub condition: Option<u32>,
    /// Index into the catalog geometries.
    pub geometry: u32,
}

/// One authored `textures` entry: the first candidate whose condition holds is drawn.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityRenderSlot {
    pub first_candidate: u32,
    pub candidate_count: u16,
    /// Offset of the candidate a default actor draws, with every query and variable at zero;
    /// absent when no candidate's condition folds to true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_candidate: Option<u16>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityRenderCandidate {
    /// Absent means the candidate applies whenever the slot is reached.
    pub condition: Option<u32>,
    /// Index into the catalog sources of the raster this candidate draws.
    pub source: u32,
}

/// A `part_visibility` rule: bones matching `pattern` (lowercase, optional trailing `*`) draw
/// only while `condition` holds; later rules override earlier ones.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityRenderVisibility {
    pub pattern: Box<str>,
    pub condition: u32,
}

/// Matches admitted bone-name patterns without allocating; ASCII case is ignored.
pub fn entity_render_pattern_matches(pattern: &str, name: &str) -> bool {
    let (leading, rest) = match pattern.strip_prefix('*') {
        Some(rest) => (true, rest),
        None => (false, pattern),
    };
    let (trailing, core) = match rest.strip_suffix('*') {
        Some(core) => (true, core),
        None => (false, rest),
    };
    let (name, core) = (name.as_bytes(), core.as_bytes());
    let at = |start: usize| {
        name.get(start..start + core.len())
            .is_some_and(|window| window.eq_ignore_ascii_case(core))
    };
    match (leading, trailing) {
        (true, true) => (0..=name.len().saturating_sub(core.len())).any(at),
        (true, false) => name.len() >= core.len() && at(name.len() - core.len()),
        (false, true) => at(0),
        (false, false) => name.eq_ignore_ascii_case(core),
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityRenderData {
    #[serde(default, skip_serializing_if = "<[_]>::is_empty")]
    pub material_selections: Box<[EntityRenderMaterialSelection]>,
    pub layers: Box<[EntityRenderLayer]>,
    pub slots: Box<[EntityRenderSlot]>,
    pub candidates: Box<[EntityRenderCandidate]>,
    pub visibility: Box<[EntityRenderVisibility]>,
    #[serde(default, skip_serializing_if = "<[_]>::is_empty")]
    pub geometries: Box<[EntityRenderGeometry]>,
    /// Resolved materials shared by layers and variants.
    #[serde(default, skip_serializing_if = "<[_]>::is_empty")]
    pub materials: Box<[super::material::EntityMaterialDefinition]>,
    /// Mask textures of Cinnabar materials, shared by the layers that draw them.
    #[serde(default, skip_serializing_if = "<[_]>::is_empty")]
    pub material_masks: Box<[EntityMaterialMasks]>,
}

/// Mask textures a Cinnabar material samples on its base texture's coordinates. Each source
/// index names an entity source; `strength` scales both masks. Equality compares exact bits.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityMaterialMasks {
    /// Added after lighting: `mask.rgb * mask.a * strength`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glow: Option<u32>,
    /// Draws masked texels unlit, weighted by `mask.a * strength`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emissive: Option<u32>,
    pub strength: f32,
}

impl PartialEq for EntityMaterialMasks {
    fn eq(&self, other: &Self) -> bool {
        (self.glow, self.emissive, self.strength.to_bits())
            == (other.glow, other.emissive, other.strength.to_bits())
    }
}

impl Eq for EntityMaterialMasks {}

pub(super) fn validate_render_payload(compiled: &CompiledEntityAssets) -> Result<(), AssetError> {
    let render = &compiled.render;
    material_selection::validate(render, compiled.molang_expressions.len())?;
    if render.layers.len() > MAX_ENTITY_RENDER_LAYERS
        || render.slots.len() > MAX_ENTITY_RENDER_SLOTS
        || render.candidates.len() > MAX_ENTITY_RENDER_CANDIDATES
        || render.geometries.len() > MAX_ENTITY_RENDER_CANDIDATES
        || render.visibility.len() > MAX_ENTITY_RENDER_VISIBILITY
        || render.materials.len() > super::material::MAX_ENTITY_MATERIAL_DEFINITIONS
    {
        return Err(invalid("entity render payload count exceeds bound"));
    }
    if !render
        .materials
        .iter()
        .all(|material| material.is_valid(render.materials.len()))
        || render.layers.iter().any(|layer| {
            layer
                .material_definition
                .is_some_and(|index| index as usize >= render.materials.len())
        })
    {
        return Err(invalid("entity material definition is invalid"));
    }
    if render.material_masks.len() > super::material::MAX_ENTITY_MATERIAL_DEFINITIONS
        || render.material_masks.iter().any(|masks| {
            !masks.strength.is_finite()
                || [masks.glow, masks.emissive]
                    .into_iter()
                    .flatten()
                    .any(|source| source as usize >= compiled.sources.len())
        })
        || render.layers.iter().any(|layer| {
            layer
                .material_masks
                .is_some_and(|index| index as usize >= render.material_masks.len())
        })
    {
        return Err(invalid("entity material masks are invalid"));
    }
    let expressions = compiled.molang_expressions.len();
    let expression = |index: u32| (index as usize) < expressions;
    let colors_valid = |color: &Option<[u32; 4]>| {
        color
            .as_ref()
            .is_none_or(|components| components.iter().all(|index| expression(*index)))
    };
    let mut previous_rig = 0;
    for layer in render.layers.iter() {
        if layer.rig < previous_rig
            || (layer.unresolved_textures && layer.slot_count != 0)
            || layer.rig as usize >= compiled.rig_bindings.len()
            || layer.condition.is_some_and(|index| !expression(index))
            || layer
                .material_selection
                .is_some_and(|index| index as usize >= render.material_selections.len())
            || !range_in_bounds(
                layer.first_slot,
                u32::from(layer.slot_count),
                render.slots.len(),
            )
            || !range_in_bounds(
                layer.first_visibility,
                u32::from(layer.visibility_count),
                render.visibility.len(),
            )
            || !colors_valid(&layer.color)
            || !colors_valid(&layer.overlay_color)
            || !colors_valid(&layer.on_fire_color)
            || !colors_valid(&layer.hurt_color)
            || !colors_valid(&layer.uv_anim)
            || layer
                .light_color_multiplier
                .is_some_and(|index| !expression(index))
            || !range_in_bounds(
                layer.first_geometry,
                u32::from(layer.geometry_count),
                render.geometries.len(),
            )
        {
            return Err(invalid("entity render layer is invalid"));
        }
        previous_rig = layer.rig;
    }
    for slot in render.slots.iter() {
        if slot.candidate_count == 0
            || slot
                .default_candidate
                .is_some_and(|offset| offset >= slot.candidate_count)
            || !range_in_bounds(
                slot.first_candidate,
                u32::from(slot.candidate_count),
                render.candidates.len(),
            )
        {
            return Err(invalid("entity render slot is invalid"));
        }
    }
    for candidate in render.candidates.iter() {
        let raster = compiled
            .sources
            .get(candidate.source as usize)
            .is_some_and(|source| {
                source.path.starts_with("textures/")
                    && (source.path.ends_with(".png") || source.path.ends_with(".tga"))
            });
        if !raster || candidate.condition.is_some_and(|index| !expression(index)) {
            return Err(invalid("entity render texture candidate is invalid"));
        }
    }
    for choice in render.geometries.iter() {
        if choice.geometry as usize >= compiled.geometries.len()
            || choice.condition.is_some_and(|index| !expression(index))
        {
            return Err(invalid("entity render geometry choice is invalid"));
        }
    }
    for rule in render.visibility.iter() {
        if rule.pattern.is_empty()
            || rule.pattern.len() > MAX_ENTITY_RENDER_PATTERN_BYTES
            || rule.pattern.chars().any(|ch| ch.is_ascii_uppercase())
            || {
                let rest = rule.pattern.strip_prefix('*').unwrap_or(&rule.pattern);
                rest.strip_suffix('*').unwrap_or(rest).contains('*')
            }
            || !expression(rule.condition)
        {
            return Err(invalid("entity render visibility rule is invalid"));
        }
    }
    validate_flattened_ranges(
        render
            .layers
            .iter()
            .map(|layer| (layer.first_slot, u32::from(layer.slot_count))),
        render.slots.len(),
        "render slot",
    )?;
    validate_flattened_ranges(
        render
            .layers
            .iter()
            .map(|layer| (layer.first_visibility, u32::from(layer.visibility_count))),
        render.visibility.len(),
        "render visibility",
    )?;
    validate_flattened_ranges(
        render
            .layers
            .iter()
            .map(|layer| (layer.first_geometry, u32::from(layer.geometry_count))),
        render.geometries.len(),
        "render geometry",
    )?;
    validate_flattened_ranges(
        render
            .slots
            .iter()
            .map(|slot| (slot.first_candidate, u32::from(slot.candidate_count))),
        render.candidates.len(),
        "render candidate",
    )
}

#[path = "render/material_selection.rs"]
mod material_selection;
pub use material_selection::EntityRenderMaterialSelection;
