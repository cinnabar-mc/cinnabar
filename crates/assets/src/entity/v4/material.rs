//! Resolved Bedrock entity materials: every recovered `.material` field after inheritance.
//!
//! The carrier keeps Bedrock's own vocabulary so reports can show what a pack authored;
//! [`EntityMaterialDefinition::render_state`] lowers it to the shared draw state.

use render_api::render_state::{
    BlendComponent, BlendFactor, BlendOperation, BlendState, CompareFunction, CullMode, DepthBias,
    DepthState, RenderState, StencilFace, StencilOperation, StencilState,
};
use serde::{Deserialize, Serialize};

/// Most definitions one carrier admits, including variants.
pub const MAX_ENTITY_MATERIAL_DEFINITIONS: usize = 16_384;
/// Longest admitted define, variant or shader identifier.
pub const MAX_ENTITY_MATERIAL_NAME_BYTES: usize = 256;
/// Sampler slots a material may configure.
pub const ENTITY_MATERIAL_SAMPLER_SLOTS: u8 = 8;

/// A recognized `states` entry.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum EntityMaterialState {
    DisableDepthTest,
    Blending,
    DisableCulling,
    DisableColorWrite,
    DisableAlphaWrite,
    DisableDepthWrite,
    StencilWrite,
    InvertCulling,
    EnableStencilTest,
    EnableAlphaToCoverage,
    /// Accepted and ignored by the Bedrock client.
    PolygonOffset,
}

impl EntityMaterialState {
    pub const ALL: [Self; 11] = [
        Self::DisableDepthTest,
        Self::Blending,
        Self::DisableCulling,
        Self::DisableColorWrite,
        Self::DisableAlphaWrite,
        Self::DisableDepthWrite,
        Self::StencilWrite,
        Self::InvertCulling,
        Self::EnableStencilTest,
        Self::EnableAlphaToCoverage,
        Self::PolygonOffset,
    ];

    /// The authored spelling.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::DisableDepthTest => "DisableDepthTest",
            Self::Blending => "Blending",
            Self::DisableCulling => "DisableCulling",
            Self::DisableColorWrite => "DisableColorWrite",
            Self::DisableAlphaWrite => "DisableAlphaWrite",
            Self::DisableDepthWrite => "DisableDepthWrite",
            Self::StencilWrite => "StencilWrite",
            Self::InvertCulling => "InvertCulling",
            Self::EnableStencilTest => "EnableStencilTest",
            Self::EnableAlphaToCoverage => "EnableAlphaToCoverage",
            Self::PolygonOffset => "PolygonOffset",
        }
    }

    /// Resolves an exact spelling.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|state| state.name() == name)
    }
}

/// Authored comparison names, used by `depthFunc` and `stencilFunc`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum EntityMaterialCompare {
    Always,
    Equal,
    NotEqual,
    Less,
    Greater,
    GreaterEqual,
    LessEqual,
}

impl EntityMaterialCompare {
    pub const ALL: [Self; 7] = [
        Self::Always,
        Self::Equal,
        Self::NotEqual,
        Self::Less,
        Self::Greater,
        Self::GreaterEqual,
        Self::LessEqual,
    ];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Always => "Always",
            Self::Equal => "Equal",
            Self::NotEqual => "NotEqual",
            Self::Less => "Less",
            Self::Greater => "Greater",
            Self::GreaterEqual => "GreaterEqual",
            Self::LessEqual => "LessEqual",
        }
    }

    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|value| value.name() == name)
    }

    #[must_use]
    pub const fn compare(self) -> CompareFunction {
        match self {
            Self::Always => CompareFunction::Always,
            Self::Equal => CompareFunction::Equal,
            Self::NotEqual => CompareFunction::NotEqual,
            Self::Less => CompareFunction::Less,
            Self::Greater => CompareFunction::Greater,
            Self::GreaterEqual => CompareFunction::GreaterEqual,
            Self::LessEqual => CompareFunction::LessEqual,
        }
    }
}

/// Authored stencil operations.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum EntityMaterialStencilOperation {
    #[default]
    Keep,
    Replace,
}

impl EntityMaterialStencilOperation {
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "Keep" => Some(Self::Keep),
            "Replace" => Some(Self::Replace),
            _ => None,
        }
    }

    #[must_use]
    pub const fn operation(self) -> StencilOperation {
        match self {
            Self::Keep => StencilOperation::Keep,
            Self::Replace => StencilOperation::Replace,
        }
    }
}

/// Authored blend factor names.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum EntityMaterialBlendFactor {
    DestColor,
    SourceColor,
    Zero,
    One,
    OneMinusDestColor,
    OneMinusSrcColor,
    SourceAlpha,
    DestAlpha,
    OneMinusSrcAlpha,
}

impl EntityMaterialBlendFactor {
    pub const ALL: [Self; 9] = [
        Self::DestColor,
        Self::SourceColor,
        Self::Zero,
        Self::One,
        Self::OneMinusDestColor,
        Self::OneMinusSrcColor,
        Self::SourceAlpha,
        Self::DestAlpha,
        Self::OneMinusSrcAlpha,
    ];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::DestColor => "DestColor",
            Self::SourceColor => "SourceColor",
            Self::Zero => "Zero",
            Self::One => "One",
            Self::OneMinusDestColor => "OneMinusDestColor",
            Self::OneMinusSrcColor => "OneMinusSrcColor",
            Self::SourceAlpha => "SourceAlpha",
            Self::DestAlpha => "DestAlpha",
            Self::OneMinusSrcAlpha => "OneMinusSrcAlpha",
        }
    }

    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|value| value.name() == name)
    }

    #[must_use]
    pub const fn factor(self) -> BlendFactor {
        match self {
            Self::DestColor => BlendFactor::DestinationColor,
            Self::SourceColor => BlendFactor::SourceColor,
            Self::Zero => BlendFactor::Zero,
            Self::One => BlendFactor::One,
            Self::OneMinusDestColor => BlendFactor::OneMinusDestinationColor,
            Self::OneMinusSrcColor => BlendFactor::OneMinusSourceColor,
            Self::SourceAlpha => BlendFactor::SourceAlpha,
            Self::DestAlpha => BlendFactor::DestinationAlpha,
            Self::OneMinusSrcAlpha => BlendFactor::OneMinusSourceAlpha,
        }
    }
}

/// One stencil facing as authored in `frontFace` or `backFace`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityMaterialStencilFace {
    pub compare: EntityMaterialCompare,
    pub fail: EntityMaterialStencilOperation,
    pub depth_fail: EntityMaterialStencilOperation,
    pub pass: EntityMaterialStencilOperation,
}

impl Default for EntityMaterialStencilFace {
    fn default() -> Self {
        Self {
            compare: EntityMaterialCompare::Always,
            fail: EntityMaterialStencilOperation::Keep,
            depth_fail: EntityMaterialStencilOperation::Keep,
            pass: EntityMaterialStencilOperation::Keep,
        }
    }
}

/// Texture filtering of one sampler slot.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum EntityMaterialFilter {
    #[default]
    Point,
    Bilinear,
    Trilinear,
    MipMapBilinear,
    TexelAA,
    PCF,
}

impl EntityMaterialFilter {
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "Point" => Self::Point,
            "Bilinear" => Self::Bilinear,
            "Trilinear" => Self::Trilinear,
            "MipMapBilinear" => Self::MipMapBilinear,
            "TexelAA" => Self::TexelAA,
            "PCF" => Self::PCF,
            _ => return None,
        })
    }
}

/// Texture addressing of one sampler slot.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum EntityMaterialWrap {
    #[default]
    Repeat,
    Clamp,
}

impl EntityMaterialWrap {
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "Repeat" => Some(Self::Repeat),
            "Clamp" => Some(Self::Clamp),
            _ => None,
        }
    }
}

/// One configured sampler slot. `stages` is the native mask: vertex 1, pixel 2, geometry 4.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityMaterialSampler {
    pub slot: u8,
    pub filter: EntityMaterialFilter,
    pub wrap: EntityMaterialWrap,
    pub stages: u8,
}

/// A declared vertex attribute.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum EntityMaterialVertexField {
    Position,
    Color,
    Normal,
    Tangent,
    UV0,
    UV1,
    UV2,
    PBRTextureIdx,
    BoneId0,
    PreviousPosition,
    HummingbirdPosition,
    HummingbirdColor,
    HummingbirdAdditional,
    Mers,
    GeoType,
}

impl EntityMaterialVertexField {
    pub const ALL: [Self; 15] = [
        Self::Position,
        Self::Color,
        Self::Normal,
        Self::Tangent,
        Self::UV0,
        Self::UV1,
        Self::UV2,
        Self::PBRTextureIdx,
        Self::BoneId0,
        Self::PreviousPosition,
        Self::HummingbirdPosition,
        Self::HummingbirdColor,
        Self::HummingbirdAdditional,
        Self::Mers,
        Self::GeoType,
    ];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Position => "Position",
            Self::Color => "Color",
            Self::Normal => "Normal",
            Self::Tangent => "Tangent",
            Self::UV0 => "UV0",
            Self::UV1 => "UV1",
            Self::UV2 => "UV2",
            Self::PBRTextureIdx => "PBRTextureIdx",
            Self::BoneId0 => "BoneId0",
            Self::PreviousPosition => "PreviousPosition",
            Self::HummingbirdPosition => "HummingbirdPosition",
            Self::HummingbirdColor => "HummingbirdColor",
            Self::HummingbirdAdditional => "HummingbirdAdditional",
            Self::Mers => "Mers",
            Self::GeoType => "GeoType",
        }
    }

    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|value| value.name() == name)
    }
}

/// Which sample counts the material supports.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum EntityMaterialMsaa {
    NonMSAA,
    MSAA,
    #[default]
    Both,
}

impl EntityMaterialMsaa {
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "NonMSAA" => Some(Self::NonMSAA),
            "MSAA" => Some(Self::MSAA),
            "Both" => Some(Self::Both),
            _ => None,
        }
    }
}

/// Authored primitive assembly.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum EntityMaterialPrimitive {
    #[default]
    None,
    QuadList,
    TriangleList,
    TriangleStrip,
    LineList,
    Line,
}

impl EntityMaterialPrimitive {
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "None" => Self::None,
            "QuadList" => Self::QuadList,
            "TriangleList" => Self::TriangleList,
            "TriangleStrip" => Self::TriangleStrip,
            "LineList" => Self::LineList,
            "Line" => Self::Line,
            _ => return None,
        })
    }
}

/// A named variant; `definition` indexes the same carrier table.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityMaterialVariant {
    pub name: Box<str>,
    pub definition: u32,
}

/// One entity material after inheritance, as the Bedrock client resolves it.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityMaterialDefinition {
    /// Recognized states in canonical order.
    pub states: Box<[EntityMaterialState]>,
    /// Shader defines in canonical order, including injected `ALPHA_TO_COVERAGE`.
    pub defines: Box<[Box<str>]>,
    pub depth_func: EntityMaterialCompare,
    /// Colour source, colour destination, alpha source and alpha destination factors.
    pub blend: [EntityMaterialBlendFactor; 4],
    pub front_face: EntityMaterialStencilFace,
    pub back_face: EntityMaterialStencilFace,
    /// `Some` when `stencilRef` overrides the draw's reference; the client keeps one byte.
    pub stencil_ref: Option<u8>,
    pub stencil_read_mask: u32,
    pub stencil_write_mask: u32,
    pub depth_bias: f32,
    pub slope_scaled_depth_bias: f32,
    pub msaa: EntityMaterialMsaa,
    pub primitive: EntityMaterialPrimitive,
    pub samplers: Box<[EntityMaterialSampler]>,
    /// `None` keeps the default vertex layout.
    pub vertex_fields: Option<Box<[EntityMaterialVertexField]>>,
    /// Built-in program identifiers; pack shader source is never compiled.
    pub vertex_shader: Option<Box<str>>,
    pub fragment_shader: Option<Box<str>>,
    pub geometry_shader: Option<Box<str>>,
    pub variants: Box<[EntityMaterialVariant]>,
}

// Admission requires finite biases, so equality is reflexive.
impl Eq for EntityMaterialDefinition {}

impl std::hash::Hash for EntityMaterialDefinition {
    /// Hashes biases by value, so `0.0` and `-0.0`, which compare equal, hash alike.
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        let bias = |value: f32| if value == 0.0 { 0 } else { value.to_bits() };
        self.states.hash(state);
        self.defines.hash(state);
        self.depth_func.hash(state);
        self.blend.hash(state);
        self.front_face.hash(state);
        self.back_face.hash(state);
        self.stencil_ref.hash(state);
        self.stencil_read_mask.hash(state);
        self.stencil_write_mask.hash(state);
        bias(self.depth_bias).hash(state);
        bias(self.slope_scaled_depth_bias).hash(state);
        self.msaa.hash(state);
        self.primitive.hash(state);
        self.samplers.hash(state);
        self.vertex_fields.hash(state);
        self.vertex_shader.hash(state);
        self.fragment_shader.hash(state);
        self.geometry_shader.hash(state);
        self.variants.hash(state);
    }
}

impl Default for EntityMaterialDefinition {
    fn default() -> Self {
        Self {
            states: Box::new([]),
            defines: Box::new([]),
            depth_func: EntityMaterialCompare::LessEqual,
            blend: [
                EntityMaterialBlendFactor::SourceAlpha,
                EntityMaterialBlendFactor::OneMinusSrcAlpha,
                EntityMaterialBlendFactor::SourceAlpha,
                EntityMaterialBlendFactor::OneMinusSrcAlpha,
            ],
            front_face: EntityMaterialStencilFace::default(),
            back_face: EntityMaterialStencilFace::default(),
            stencil_ref: None,
            stencil_read_mask: u32::from(u8::MAX),
            stencil_write_mask: u32::from(u8::MAX),
            depth_bias: 0.0,
            slope_scaled_depth_bias: 0.0,
            msaa: EntityMaterialMsaa::Both,
            primitive: EntityMaterialPrimitive::None,
            samplers: Box::new([]),
            vertex_fields: None,
            vertex_shader: None,
            fragment_shader: None,
            geometry_shader: None,
            variants: Box::new([]),
        }
    }
}

impl EntityMaterialDefinition {
    /// Whether the material sets `state`.
    #[must_use]
    pub fn has_state(&self, state: EntityMaterialState) -> bool {
        self.states.contains(&state)
    }

    /// Whether the material defines `name`.
    #[must_use]
    pub fn has_define(&self, name: &str) -> bool {
        self.defines.iter().any(|define| define.as_ref() == name)
    }

    /// Lowers the authored fixed-function fields to the shared draw state.
    #[must_use]
    pub fn render_state(&self) -> RenderState {
        use EntityMaterialState as State;
        let component = |source: EntityMaterialBlendFactor,
                         destination: EntityMaterialBlendFactor| {
            BlendComponent {
                source: source.factor(),
                destination: destination.factor(),
                operation: BlendOperation::Add,
            }
        };
        let face = |face: EntityMaterialStencilFace| StencilFace {
            compare: face.compare.compare(),
            fail: face.fail.operation(),
            depth_fail: face.depth_fail.operation(),
            pass: face.pass.operation(),
        };
        RenderState {
            cull: if self.has_state(State::DisableCulling) {
                CullMode::None
            } else if self.has_state(State::InvertCulling) {
                CullMode::Front
            } else {
                CullMode::Back
            },
            depth: DepthState {
                compare: (!self.has_state(State::DisableDepthTest))
                    .then(|| self.depth_func.compare()),
                write: !self.has_state(State::DisableDepthWrite),
                bias: DepthBias {
                    constant: self.depth_bias,
                    slope_scale: self.slope_scaled_depth_bias,
                },
            },
            stencil: self
                .has_state(State::EnableStencilTest)
                .then(|| StencilState {
                    front: face(self.front_face),
                    back: face(self.back_face),
                    reference: self.stencil_ref.unwrap_or(0),
                    read_mask: self.stencil_read_mask,
                    write_mask: self.stencil_write_mask,
                }),
            blend: self.has_state(State::Blending).then(|| BlendState {
                color: component(self.blend[0], self.blend[1]),
                alpha: component(self.blend[2], self.blend[3]),
            }),
            write_color: !self.has_state(State::DisableColorWrite),
            // Bedrock clears the whole colour write mask, alpha included, for DisableColorWrite.
            write_alpha: !self.has_state(State::DisableColorWrite)
                && !self.has_state(State::DisableAlphaWrite),
            alpha_to_coverage: self.has_state(State::EnableAlphaToCoverage),
        }
    }

    /// Bounds, ordering and index checks for a carrier table of `count` definitions.
    pub(crate) fn is_valid(&self, count: usize) -> bool {
        let name = |value: &str| !value.is_empty() && value.len() <= MAX_ENTITY_MATERIAL_NAME_BYTES;
        self.states.is_sorted_by(|a, b| a < b)
            && self.defines.is_sorted_by(|a, b| a < b)
            && self.defines.iter().all(|define| name(define))
            && [
                &self.vertex_shader,
                &self.fragment_shader,
                &self.geometry_shader,
            ]
            .into_iter()
            .flatten()
            .all(|shader| name(shader))
            && self.depth_bias.is_finite()
            && self.slope_scaled_depth_bias.is_finite()
            && self.samplers.is_sorted_by(|a, b| a.slot < b.slot)
            && self
                .samplers
                .iter()
                .all(|sampler| sampler.slot < ENTITY_MATERIAL_SAMPLER_SLOTS && sampler.stages <= 7)
            && self
                .variants
                .iter()
                .all(|variant| name(&variant.name) && (variant.definition as usize) < count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_and_blend_lower_to_shared_draw_state() {
        let definition = EntityMaterialDefinition {
            states: Box::new([
                EntityMaterialState::Blending,
                EntityMaterialState::DisableAlphaWrite,
                EntityMaterialState::InvertCulling,
                EntityMaterialState::EnableStencilTest,
            ]),
            blend: [
                EntityMaterialBlendFactor::One,
                EntityMaterialBlendFactor::One,
                EntityMaterialBlendFactor::Zero,
                EntityMaterialBlendFactor::One,
            ],
            stencil_ref: Some(3),
            ..Default::default()
        };
        let state = definition.render_state();
        assert_eq!(state.cull, CullMode::Front);
        assert!(state.write_color && !state.write_alpha);
        assert_eq!(state.stencil.map(|stencil| stencil.reference), Some(3));
        let blend = state.blend.unwrap();
        assert_eq!(blend.color.source, BlendFactor::One);
        assert_eq!(blend.alpha.source, BlendFactor::Zero);

        let untested = EntityMaterialDefinition {
            states: Box::new([EntityMaterialState::DisableDepthTest]),
            ..Default::default()
        };
        assert_eq!(untested.render_state().depth.compare, None);
        assert!(
            EntityMaterialDefinition::default()
                .render_state()
                .blend
                .is_none()
        );
    }

    #[test]
    fn disabling_colour_writes_also_masks_alpha() {
        use EntityMaterialState as State;
        for (states, color, alpha) in [
            (&[][..], true, true),
            (&[State::DisableAlphaWrite][..], true, false),
            (&[State::DisableColorWrite][..], false, false),
            (
                &[State::DisableColorWrite, State::DisableAlphaWrite][..],
                false,
                false,
            ),
        ] {
            let state = EntityMaterialDefinition {
                states: states.into(),
                ..Default::default()
            }
            .render_state();
            assert_eq!((state.write_color, state.write_alpha), (color, alpha));
        }
    }

    #[test]
    fn spellings_round_trip() {
        for state in EntityMaterialState::ALL {
            assert_eq!(EntityMaterialState::from_name(state.name()), Some(state));
        }
        for field in EntityMaterialVertexField::ALL {
            assert_eq!(
                EntityMaterialVertexField::from_name(field.name()),
                Some(field)
            );
        }
        for factor in EntityMaterialBlendFactor::ALL {
            assert_eq!(
                EntityMaterialBlendFactor::from_name(factor.name()),
                Some(factor)
            );
        }
    }
}
