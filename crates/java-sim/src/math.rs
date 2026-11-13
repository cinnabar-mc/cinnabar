//! Java Edition 1.7.10 arithmetic helpers: the sine table, flooring and the axis-aligned box.
//!
//! Positions and motion are `f64`; angles, speeds and friction are `f32`, widened only where
//! Java widens them, so results match the game bit for bit.

use std::sync::OnceLock;

/// Scale from radians to a sine-table index.
const SIN_INDEX_SCALE: f32 = 10430.378;
/// Table offset of a quarter turn, which turns a sine lookup into a cosine lookup.
const COS_INDEX_OFFSET: f32 = 16384.0;
const SIN_TABLE_LEN: usize = 65536;

/// The game's 65536-entry sine table: entry `i` is `sin(i * 2π / 65536)` rounded to `f32`.
pub fn sin_table() -> &'static [f32] {
    static TABLE: OnceLock<Vec<f32>> = OnceLock::new();
    TABLE.get_or_init(|| {
        (0..SIN_TABLE_LEN)
            .map(|index| (index as f64 * std::f64::consts::PI * 2.0 / 65536.0).sin() as f32)
            .collect()
    })
}

/// Table sine of `radians`: the index truncates toward zero and wraps to 16 bits.
#[must_use]
pub fn sin(radians: f32) -> f32 {
    sin_table()[((radians * SIN_INDEX_SCALE) as i32 & 0xFFFF) as usize]
}

/// Table cosine of `radians`, a quarter turn ahead of [`sin`].
#[must_use]
pub fn cos(radians: f32) -> f32 {
    sin_table()[((radians * SIN_INDEX_SCALE + COS_INDEX_OFFSET) as i32 & 0xFFFF) as usize]
}

/// Largest integer not above `value`, saturating like a Java `(int)` cast.
#[must_use]
pub fn floor(value: f64) -> i32 {
    let truncated = value as i32;
    if value < f64::from(truncated) {
        truncated - 1
    } else {
        truncated
    }
}

/// Square root of an `f64`, rounded to `f32` as the game stores it.
#[must_use]
pub fn sqrt(value: f64) -> f32 {
    value.sqrt() as f32
}

/// An axis-aligned box in world coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min_x: f64,
    pub min_y: f64,
    pub min_z: f64,
    pub max_x: f64,
    pub max_y: f64,
    pub max_z: f64,
}

impl Aabb {
    /// A box from its two corners; callers pass the minimum corner first.
    #[must_use]
    pub const fn new(
        min_x: f64,
        min_y: f64,
        min_z: f64,
        max_x: f64,
        max_y: f64,
        max_z: f64,
    ) -> Self {
        Self {
            min_x,
            min_y,
            min_z,
            max_x,
            max_y,
            max_z,
        }
    }

    /// Moves the box in place.
    pub fn offset(&mut self, x: f64, y: f64, z: f64) {
        self.min_x += x;
        self.min_y += y;
        self.min_z += z;
        self.max_x += x;
        self.max_y += y;
        self.max_z += z;
    }

    /// A moved copy of the box.
    #[must_use]
    pub fn offset_by(&self, x: f64, y: f64, z: f64) -> Self {
        Self::new(
            self.min_x + x,
            self.min_y + y,
            self.min_z + z,
            self.max_x + x,
            self.max_y + y,
            self.max_z + z,
        )
    }

    /// The box stretched along a motion vector: each face moves only in its own direction.
    #[must_use]
    pub fn add_coord(&self, x: f64, y: f64, z: f64) -> Self {
        let mut stretched = *self;
        if x < 0.0 {
            stretched.min_x += x;
        }
        if x > 0.0 {
            stretched.max_x += x;
        }
        if y < 0.0 {
            stretched.min_y += y;
        }
        if y > 0.0 {
            stretched.max_y += y;
        }
        if z < 0.0 {
            stretched.min_z += z;
        }
        if z > 0.0 {
            stretched.max_z += z;
        }
        stretched
    }

    /// The box grown by a margin on every face; negative margins shrink it.
    #[must_use]
    pub fn expand(&self, x: f64, y: f64, z: f64) -> Self {
        Self::new(
            self.min_x - x,
            self.min_y - y,
            self.min_z - z,
            self.max_x + x,
            self.max_y + y,
            self.max_z + z,
        )
    }

    /// The box shrunk by a margin on every face.
    #[must_use]
    pub fn contract(&self, x: f64, y: f64, z: f64) -> Self {
        Self::new(
            self.min_x + x,
            self.min_y + y,
            self.min_z + z,
            self.max_x - x,
            self.max_y - y,
            self.max_z - z,
        )
    }

    /// Whether the boxes overlap with positive volume; touching faces do not intersect.
    #[must_use]
    pub fn intersects(&self, other: &Self) -> bool {
        if other.max_x <= self.min_x || other.min_x >= self.max_x {
            return false;
        }
        if other.max_y <= self.min_y || other.min_y >= self.max_y {
            return false;
        }
        if other.max_z <= self.min_z || other.min_z >= self.max_z {
            return false;
        }
        true
    }

    /// Clips `mover`'s motion along X against this obstacle; `distance` passes through when the
    /// boxes do not overlap on the other two axes.
    #[must_use]
    pub fn clip_x(&self, mover: &Self, mut distance: f64) -> f64 {
        if mover.max_y <= self.min_y || mover.min_y >= self.max_y {
            return distance;
        }
        if mover.max_z <= self.min_z || mover.min_z >= self.max_z {
            return distance;
        }
        if distance > 0.0 && mover.max_x <= self.min_x {
            let gap = self.min_x - mover.max_x;
            if gap < distance {
                distance = gap;
            }
        }
        if distance < 0.0 && mover.min_x >= self.max_x {
            let gap = self.max_x - mover.min_x;
            if gap > distance {
                distance = gap;
            }
        }
        distance
    }

    /// Clips `mover`'s motion along Y against this obstacle.
    #[must_use]
    pub fn clip_y(&self, mover: &Self, mut distance: f64) -> f64 {
        if mover.max_x <= self.min_x || mover.min_x >= self.max_x {
            return distance;
        }
        if mover.max_z <= self.min_z || mover.min_z >= self.max_z {
            return distance;
        }
        if distance > 0.0 && mover.max_y <= self.min_y {
            let gap = self.min_y - mover.max_y;
            if gap < distance {
                distance = gap;
            }
        }
        if distance < 0.0 && mover.min_y >= self.max_y {
            let gap = self.max_y - mover.min_y;
            if gap > distance {
                distance = gap;
            }
        }
        distance
    }

    /// Clips `mover`'s motion along Z against this obstacle.
    #[must_use]
    pub fn clip_z(&self, mover: &Self, mut distance: f64) -> f64 {
        if mover.max_x <= self.min_x || mover.min_x >= self.max_x {
            return distance;
        }
        if mover.max_y <= self.min_y || mover.min_y >= self.max_y {
            return distance;
        }
        if distance > 0.0 && mover.max_z <= self.min_z {
            let gap = self.min_z - mover.max_z;
            if gap < distance {
                distance = gap;
            }
        }
        if distance < 0.0 && mover.min_z >= self.max_z {
            let gap = self.max_z - mover.min_z;
            if gap > distance {
                distance = gap;
            }
        }
        distance
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floor_rounds_negative_fractions_down() {
        assert_eq!(floor(-0.5), -1);
        assert_eq!(floor(-1.0), -1);
        assert_eq!(floor(1.999), 1);
        assert_eq!(floor(f64::NAN), 0);
        assert_eq!(floor(1e300), i32::MAX);
    }

    #[test]
    fn touching_boxes_do_not_intersect() {
        let unit = Aabb::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);
        assert!(!unit.intersects(&unit.offset_by(1.0, 0.0, 0.0)));
        assert!(unit.intersects(&unit.offset_by(0.999, 0.0, 0.0)));
    }

    #[test]
    fn clip_stops_at_the_obstacle_face() {
        let obstacle = Aabb::new(2.0, 0.0, 0.0, 3.0, 1.0, 1.0);
        let mover = Aabb::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);
        assert_eq!(obstacle.clip_x(&mover, 5.0), 1.0);
        assert_eq!(obstacle.clip_x(&mover.offset_by(0.0, 1.0, 0.0), 5.0), 5.0);
    }
}
