//! Native fishing-hook event effects: the approaching fish's trail, the bite and the tease.
//!
//! The server sends only the hook's event; every client draws these effects itself.

use assets::facts::ParticleEffect;

use crate::{SpawnRequest, molang::Rng, native_request};

/// Rise above the hook box's floored bottom where the bite effects start.
const BITE_RISE: f32 = 1.0;
/// Multiple of the hook's width added to the bite direction's x component.
const BITE_WIDTH_SCALE: f32 = 20.0;
/// Rise above the hook box's floored bottom where the approaching fish's trail starts.
const TRAIL_RISE: f32 = 0.9;
/// Chance that one trail event also releases a fish bubble.
const TRAIL_BUBBLE_CHANCE: f32 = 0.15;
/// Vertical component of a trail bubble's direction.
const TRAIL_BUBBLE_LIFT: f32 = 0.1;
/// Scale of the two trail wakes' sideways directions.
const TRAIL_WAKE_SPREAD: f32 = 0.004;
/// Vertical component of each trail wake's direction.
const TRAIL_WAKE_LIFT: f32 = 0.01;
/// Direction of every tease splash.
const TEASE_DIRECTION: [f32; 3] = [0.1; 3];
/// Entries in the native sine table, which holds one turn.
const SINE_TABLE_LEN: i32 = 65536;
/// Native sine table entries per radian.
const SINE_TABLE_SCALE: f32 = 10430.378;
/// Sine table entries in a quarter turn, which turn a sine lookup into a cosine.
const SINE_TABLE_QUARTER: f32 = 16384.0;

/// Native table sine of `entries`, a table position that wraps every table length.
fn table_sine(entries: f32) -> f32 {
    let index = (entries as i32) & (SINE_TABLE_LEN - 1);
    (index as f32 * std::f32::consts::TAU / SINE_TABLE_LEN as f32).sin()
}

/// Spawn request for `effect` with its `variable.direction` set.
fn directed(effect: ParticleEffect, position: [f32; 3], direction: [f32; 3]) -> SpawnRequest {
    SpawnRequest {
        variables: ["direction.x", "direction.y", "direction.z"]
            .into_iter()
            .zip(direction)
            .map(|(name, value)| (name.to_owned(), value))
            .collect(),
        ..native_request(effect, position, None)
    }
}

/// Bite effects for a hook at `position` whose collision box starts at `bottom` and is `width`
/// wide: one hook bubble and two wakes, all leaving one block above the floored box bottom.
#[must_use]
pub fn fishing_bite_requests(position: [f32; 3], bottom: f32, width: f32) -> [SpawnRequest; 3] {
    let origin = [position[0], bottom.floor() + BITE_RISE, position[2]];
    let direction = [BITE_WIDTH_SCALE * width + 1.0, 0.0, width];
    [
        directed(ParticleEffect::FishHookParticle, origin, direction),
        // Native emits the wake twice: once by legacy particle type, once by name.
        directed(ParticleEffect::WaterWakeParticle, origin, direction),
        directed(ParticleEffect::WaterWakeParticle, origin, direction),
    ]
}

/// Trail effects of a fish `offset` (x, z) from a hook at `position` with box bottom `bottom`,
/// approaching at `angle` radians: two wakes, and a bubble when `seed`'s roll passes the chance.
#[must_use]
pub fn fish_trail_requests(
    position: [f32; 3],
    bottom: f32,
    offset: [f32; 2],
    angle: f32,
    seed: u64,
) -> Vec<SpawnRequest> {
    let origin = [
        position[0] + offset[0],
        bottom.floor() + TRAIL_RISE,
        position[2] + offset[1],
    ];
    let entries = angle * SINE_TABLE_SCALE;
    let (sin, cos) = (
        table_sine(entries),
        table_sine(entries + SINE_TABLE_QUARTER),
    );
    let mut requests = Vec::with_capacity(3);
    if Rng::new(seed).unit() < TRAIL_BUBBLE_CHANCE {
        requests.push(directed(
            ParticleEffect::FishPosParticle,
            origin,
            [sin, TRAIL_BUBBLE_LIFT, cos],
        ));
    }
    let (sin, cos) = (sin * TRAIL_WAKE_SPREAD, cos * TRAIL_WAKE_SPREAD);
    for direction in [[-cos, TRAIL_WAKE_LIFT, sin], [cos, TRAIL_WAKE_LIFT, -sin]] {
        requests.push(directed(
            ParticleEffect::WaterWakeParticle,
            origin,
            direction,
        ));
    }
    requests
}

/// Block a tease checks for water: the fish's column at the hook box's floored bottom.
/// `None` when the hook state is not finite.
#[must_use]
pub fn fish_tease_cell(position: [f32; 3], bottom: f32, offset: [f32; 2]) -> Option<[i32; 3]> {
    let cell = [
        position[0] + offset[0],
        bottom.floor(),
        position[2] + offset[1],
    ];
    cell.iter()
        .all(|axis| axis.is_finite())
        .then(|| cell.map(|axis| axis.floor() as i32))
}

/// Splash that starts at the teased water `cell`'s minimum corner.
#[must_use]
pub fn fish_tease_request(cell: [i32; 3]) -> SpawnRequest {
    directed(
        ParticleEffect::WaterSplashParticle,
        cell.map(|axis| axis as f32),
        TEASE_DIRECTION,
    )
}

#[cfg(test)]
#[path = "fishing_tests.rs"]
mod tests;
