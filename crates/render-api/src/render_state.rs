//! Fixed-function draw state shared by every material source.
//!
//! Bedrock materials, Cinnabar's declarative extension and Experience materials all lower to
//! [`RenderState`]. The renderer alone maps it to GPU pipeline state. Depth comparisons use the
//! authored forward convention (nearer is less); the renderer adapts them to its depth buffer.

/// Which faces are discarded before rasterization.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CullMode {
    None,
    Front,
    #[default]
    Back,
}

/// A depth or stencil comparison against the stored value.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CompareFunction {
    Never,
    Less,
    Equal,
    LessEqual,
    Greater,
    NotEqual,
    GreaterEqual,
    Always,
}

/// Constant and slope-scaled depth offsets, as authored. Equality compares the exact bits.
#[derive(Clone, Copy, Debug, Default)]
pub struct DepthBias {
    pub constant: f32,
    pub slope_scale: f32,
}

impl PartialEq for DepthBias {
    fn eq(&self, other: &Self) -> bool {
        self.constant.to_bits() == other.constant.to_bits()
            && self.slope_scale.to_bits() == other.slope_scale.to_bits()
    }
}

impl Eq for DepthBias {}

impl PartialOrd for DepthBias {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Orders by the exact bits, consistent with equality.
impl Ord for DepthBias {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.constant.to_bits(), self.slope_scale.to_bits())
            .cmp(&(other.constant.to_bits(), other.slope_scale.to_bits()))
    }
}

impl std::hash::Hash for DepthBias {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.constant.to_bits().hash(state);
        self.slope_scale.to_bits().hash(state);
    }
}

/// Depth testing and writing.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DepthState {
    /// `None` disables the depth test entirely.
    pub compare: Option<CompareFunction>,
    pub write: bool,
    pub bias: DepthBias,
}

impl Default for DepthState {
    fn default() -> Self {
        Self {
            compare: Some(CompareFunction::LessEqual),
            write: true,
            bias: DepthBias::default(),
        }
    }
}

/// What a stencil test outcome does to the stored value.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum StencilOperation {
    #[default]
    Keep,
    Zero,
    Replace,
    Invert,
    IncrementClamp,
    DecrementClamp,
    IncrementWrap,
    DecrementWrap,
}

/// The stencil test and its three outcomes for one facing.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct StencilFace {
    pub compare: CompareFunction,
    pub fail: StencilOperation,
    pub depth_fail: StencilOperation,
    pub pass: StencilOperation,
}

impl Default for StencilFace {
    fn default() -> Self {
        Self {
            compare: CompareFunction::Always,
            fail: StencilOperation::Keep,
            depth_fail: StencilOperation::Keep,
            pass: StencilOperation::Keep,
        }
    }
}

/// Stencil testing; `None` in [`RenderState::stencil`] disables it.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct StencilState {
    pub front: StencilFace,
    pub back: StencilFace,
    pub reference: u8,
    pub read_mask: u32,
    pub write_mask: u32,
}

impl Default for StencilState {
    fn default() -> Self {
        Self {
            front: StencilFace::default(),
            back: StencilFace::default(),
            reference: 0,
            read_mask: u32::MAX,
            write_mask: u32::MAX,
        }
    }
}

/// A blend factor applied to the source or destination.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum BlendFactor {
    Zero,
    One,
    SourceColor,
    OneMinusSourceColor,
    SourceAlpha,
    OneMinusSourceAlpha,
    DestinationColor,
    OneMinusDestinationColor,
    DestinationAlpha,
    OneMinusDestinationAlpha,
}

/// How weighted source and destination combine.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum BlendOperation {
    #[default]
    Add,
    Subtract,
    ReverseSubtract,
    Min,
    Max,
}

/// Blending for one set of channels.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BlendComponent {
    pub source: BlendFactor,
    pub destination: BlendFactor,
    pub operation: BlendOperation,
}

/// Independent colour and alpha blending.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BlendState {
    pub color: BlendComponent,
    pub alpha: BlendComponent,
}

impl BlendState {
    /// Conventional alpha blending: `source * a + destination * (1 - a)`.
    pub const ALPHA: Self = Self {
        color: BlendComponent {
            source: BlendFactor::SourceAlpha,
            destination: BlendFactor::OneMinusSourceAlpha,
            operation: BlendOperation::Add,
        },
        alpha: BlendComponent {
            source: BlendFactor::SourceAlpha,
            destination: BlendFactor::OneMinusSourceAlpha,
            operation: BlendOperation::Add,
        },
    };
}

/// Complete fixed-function state of one draw.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RenderState {
    pub cull: CullMode,
    pub depth: DepthState,
    pub stencil: Option<StencilState>,
    /// `None` writes the source unblended.
    pub blend: Option<BlendState>,
    pub write_color: bool,
    pub write_alpha: bool,
    pub alpha_to_coverage: bool,
}

impl Default for RenderState {
    fn default() -> Self {
        Self {
            cull: CullMode::Back,
            depth: DepthState::default(),
            stencil: None,
            blend: None,
            write_color: true,
            write_alpha: true,
            alpha_to_coverage: false,
        }
    }
}

impl RenderState {
    /// Whether draws pass regardless of stored depth: the test is disabled or always passes.
    #[must_use]
    pub fn ignores_stored_depth(&self) -> bool {
        matches!(self.depth.compare, None | Some(CompareFunction::Always))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depth_bias_equality_distinguishes_every_bit() {
        let positive = DepthBias {
            constant: 0.0,
            slope_scale: 1.0,
        };
        let negative_zero = DepthBias {
            constant: -0.0,
            ..positive
        };
        assert_eq!(positive, positive);
        assert_ne!(positive, negative_zero);
    }
}
