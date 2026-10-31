//! Vanilla lit-candle smoke and flame origins; the loaded particle pack defines their art and
//! motion.

use super::AmbientRandom;

pub const CANDLE_SMOKE_EFFECT: &str =
    assets::facts::ParticleEffect::BasicSmokeParticle.identifier();
pub const CANDLE_FLAME_EFFECT: &str =
    assets::facts::ParticleEffect::CandleFlameParticle.identifier();
const SMOKE_CHANCE: f32 = 0.3;
const EFFECT_LIFT: f32 = 0.125;

/// Emits one callback's effects for the candles at `origins` inside `block`. Each candle takes
/// one roll from the sampler's shared stream, may smoke, and always shows a flame.
pub fn emit_candle_effects(
    block: [i32; 3],
    origins: impl IntoIterator<Item = [f32; 3]>,
    random: &mut AmbientRandom,
    mut emit: impl FnMut(&'static str, [f32; 3]),
) {
    for origin in origins {
        let position = [
            block[0] as f32 + origin[0],
            block[1] as f32 + origin[1] + EFFECT_LIFT,
            block[2] as f32 + origin[2],
        ];
        if random.unit() < SMOKE_CHANCE {
            emit(CANDLE_SMOKE_EFFECT, position);
        }
        emit(CANDLE_FLAME_EFFECT, position);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_candle_rolls_once_and_always_shows_a_flame_above_itself() {
        let origins = [[0.5625, 0.375, 0.375], [0.4375, 0.1875, 0.5625]];
        for seed in 0..64 {
            let mut rolls = AmbientRandom::new(seed);
            let mut expected = Vec::new();
            for (origin, height) in origins.iter().zip([0.5, 0.3125]) {
                let position = [-4.0 + origin[0], 70.0 + height, 9.0 + origin[2]];
                if rolls.unit() < SMOKE_CHANCE {
                    expected.push((CANDLE_SMOKE_EFFECT, position));
                }
                expected.push((CANDLE_FLAME_EFFECT, position));
            }
            let mut random = AmbientRandom::new(seed);
            let mut emitted = Vec::new();
            emit_candle_effects([-4, 70, 9], origins, &mut random, |effect, position| {
                emitted.push((effect, position));
            });
            assert_eq!(emitted, expected);
            assert_eq!(random.next(), rolls.next(), "one roll per candle");
        }
    }

    #[test]
    fn an_unlit_block_consumes_no_rolls() {
        let mut random = AmbientRandom::new(7);
        emit_candle_effects([0; 3], [], &mut random, |effect, _| {
            panic!("unexpected {effect}");
        });
        assert_eq!(random.next(), AmbientRandom::new(7).next());
    }
}
