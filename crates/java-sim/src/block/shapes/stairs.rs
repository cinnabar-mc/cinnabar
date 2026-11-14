//! Stairs: a half-block slab, the step on it, and the extra quarter of an inner corner.
//!
//! The low two metadata bits name the direction the step rises towards (0 east, 1 west, 2 south,
//! 3 north) and bit 4 flips the stairs upside down. Corners form against stairs of the same half
//! in front of or behind the step, unless the block beside this one on the corner's side is a
//! stair with exactly this block's metadata.

use super::{Bounds, Collector, Shape, meta_of};
use crate::block::facts::entry_of;
use crate::world::World;

/// A stair block and the world around it.
struct Site<'a, W> {
    world: &'a W,
    x: i32,
    y: i32,
    z: i32,
    /// The stair's whole metadata.
    meta: i32,
}

impl<W: World> Site<'_, W> {
    /// The metadata of the block offset by `(dx, dz)` when it is a stair block of any kind.
    fn stairs_meta(&self, (dx, dz): (i32, i32)) -> Option<i32> {
        let block = self
            .world
            .block(self.x.wrapping_add(dx), self.y, self.z.wrapping_add(dz));
        (entry_of(block.id).shape == Shape::Stairs).then(|| meta_of(block))
    }

    /// The facing of the stair offset by `offset` when it sits in the same half as this one.
    fn aligned_facing(&self, offset: (i32, i32)) -> Option<i32> {
        let meta = self.stairs_meta(offset)?;
        (meta & 4 == self.meta & 4).then_some(meta & 3)
    }

    /// Whether the block offset by `offset` is a stair with exactly this stair's metadata.
    fn twin(&self, offset: (i32, i32)) -> bool {
        self.stairs_meta(offset) == Some(self.meta)
    }

    /// The vertical extent of the step: the half the slab does not fill.
    fn step_height(&self) -> (f32, f32) {
        if self.meta & 4 != 0 {
            (0.0, 0.5)
        } else {
            (0.5, 1.0)
        }
    }

    /// The step box and whether the stair is straight. An outer corner, formed against a turned
    /// stair in front of the step, shrinks the step to a quarter and is not straight.
    fn step(&self) -> (Bounds, bool) {
        let facing = self.meta & 3;
        let (min_y, max_y) = self.step_height();
        let (mut min_x, mut max_x, mut min_z, mut max_z): (f32, f32, f32, f32) = match facing {
            0 => (0.5, 1.0, 0.0, 1.0),
            1 => (0.0, 0.5, 0.0, 1.0),
            2 => (0.0, 1.0, 0.5, 1.0),
            _ => (0.0, 1.0, 0.0, 0.5),
        };
        let front = match facing {
            0 => (1, 0),
            1 => (-1, 0),
            2 => (0, 1),
            _ => (0, -1),
        };
        let mut straight = false;
        match self.aligned_facing(front) {
            Some(3) if facing < 2 && !self.twin((0, 1)) => max_z = 0.5,
            Some(2) if facing < 2 && !self.twin((0, -1)) => min_z = 0.5,
            Some(1) if facing >= 2 && !self.twin((1, 0)) => max_x = 0.5,
            Some(0) if facing >= 2 && !self.twin((-1, 0)) => min_x = 0.5,
            _ => straight = true,
        }
        (
            Bounds::new(min_x, min_y, min_z, max_x, max_y, max_z),
            straight,
        )
    }

    /// The extra quarter of an inner corner, formed against a turned stair behind the step.
    fn inner_corner(&self) -> Option<Bounds> {
        let facing = self.meta & 3;
        let (min_y, max_y) = self.step_height();
        let back = match facing {
            0 => (-1, 0),
            1 => (1, 0),
            2 => (0, -1),
            _ => (0, 1),
        };
        let turn = self.aligned_facing(back)?;
        let (min_x, max_x, min_z, max_z): (f32, f32, f32, f32) = if facing < 2 {
            let (min_x, max_x) = if facing == 1 { (0.5, 1.0) } else { (0.0, 0.5) };
            match turn {
                3 if !self.twin((0, -1)) => (min_x, max_x, 0.0, 0.5),
                2 if !self.twin((0, 1)) => (min_x, max_x, 0.5, 1.0),
                _ => return None,
            }
        } else {
            let (min_z, max_z) = if facing == 2 { (0.0, 0.5) } else { (0.5, 1.0) };
            match turn {
                1 if !self.twin((-1, 0)) => (0.0, 0.5, min_z, max_z),
                0 if !self.twin((1, 0)) => (0.5, 1.0, min_z, max_z),
                _ => return None,
            }
        };
        Some(Bounds::new(min_x, min_y, min_z, max_x, max_y, max_z))
    }
}

/// Stairs: the slab half, then the step, then the inner-corner quarter when the stair is straight
/// along its own axis but meets a turned stair behind it.
pub(super) fn stairs(boxes: &mut Collector<'_>, world: &impl World, meta: i32) {
    let site = Site {
        world,
        x: boxes.x,
        y: boxes.y,
        z: boxes.z,
        meta,
    };
    boxes.local(if meta & 4 != 0 {
        Bounds::new(0.0, 0.5, 0.0, 1.0, 1.0, 1.0)
    } else {
        Bounds::new(0.0, 0.0, 0.0, 1.0, 0.5, 1.0)
    });
    let (step, straight) = site.step();
    boxes.local(step);
    if straight && let Some(corner) = site.inner_corner() {
        boxes.local(corner);
    }
}
