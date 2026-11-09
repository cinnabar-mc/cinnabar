//! Vanilla ender chest origins; the loaded particle pack defines their art and motion.

use super::AmbientRandom;

pub const ENDER_CHEST_EFFECT: &str = assets::facts::ParticleEffect::EndChest.identifier();
const EFFECTS_PER_CALLBACK: usize = 30;
const CENTER: f32 = 0.5;
const SIDE_OFFSET: f32 = 0.25;

/// Emits one callback's effects for the ender chest at `block`. Each effect starts a quarter
/// block off the centre toward a random corner, at a random height inside the block, and
/// takes three rolls from the sampler's shared stream: the X side, the Z side, then the height.
pub fn emit_ender_chest_effects(
    block: [i32; 3],
    random: &mut AmbientRandom,
    mut emit: impl FnMut([f32; 3]),
) {
    let base = block.map(|component| component as f32);
    for _ in 0..EFFECTS_PER_CALLBACK {
        let x_side = side(random);
        let z_side = side(random);
        let height = random.unit();
        emit([
            base[0] + CENTER + x_side * SIDE_OFFSET,
            base[1] + height,
            base[2] + CENTER + z_side * SIDE_OFFSET,
        ]);
    }
}

/// One roll choosing the negative (-1) or positive (+1) side of the block's centre.
fn side(random: &mut AmbientRandom) -> f32 {
    (random.bounded(2) as i32 * 2 - 1) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_callback_emits_thirty_effects_from_the_quarter_corners_at_random_heights() {
        for seed in 0..64 {
            let mut rolls = AmbientRandom::new(seed);
            let mut expected = Vec::new();
            for _ in 0..30 {
                let x = if rolls.next().is_multiple_of(2) {
                    -0.25
                } else {
                    0.25
                };
                let z = if rolls.next().is_multiple_of(2) {
                    -0.25
                } else {
                    0.25
                };
                let y = rolls.unit();
                expected.push([-4.0 + 0.5 + x, 70.0 + y, 9.0 + 0.5 + z]);
            }
            let mut random = AmbientRandom::new(seed);
            let mut emitted = Vec::new();
            emit_ender_chest_effects([-4, 70, 9], &mut random, |position| {
                emitted.push(position);
            });
            assert_eq!(emitted, expected);
            assert_eq!(random.next(), rolls.next(), "three rolls per effect");
        }
    }

    #[test]
    fn effects_stay_inside_the_chest_column() {
        let mut random = AmbientRandom::new(11);
        emit_ender_chest_effects([0, 64, 0], &mut random, |[x, y, z]| {
            assert!(x == 0.25 || x == 0.75, "{x}");
            assert!(z == 0.25 || z == 0.75, "{z}");
            assert!((64.0..=65.0).contains(&y), "{y}");
        });
    }
}
