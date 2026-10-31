//! Native ambient sampling, its shared random stream, and leaf/fire/candle admission.

use assets::native_material::NativeMaterialFacts;

mod candle;
mod fire;
mod random;
mod sampler;
#[cfg(test)]
mod tests;

pub use candle::{CANDLE_FLAME_EFFECT, CANDLE_SMOKE_EFFECT, emit_candle_effects};
pub use fire::{FIRE_SMOKE_EFFECT, emit_fire_smoke};
pub use random::AmbientRandom;
pub use sampler::{MIN_SAMPLES, SamplePlan, Sampler};

pub const LEAF_EFFECT: &str = assets::facts::ParticleEffect::BiomeTintedLeavesParticle.identifier();
pub const LEAF_CHANCE_DENOMINATOR: u32 = 100;

/// Reads the admitted solid/liquid disposition without inspecting block names or geometry.
#[must_use]
pub const fn material_allows_leaf(material: NativeMaterialFacts) -> bool {
    matches!(
        material,
        NativeMaterialFacts::NonSolidNonLiquid | NativeMaterialFacts::ProvisionalNonSolidNonLiquid
    )
}
