//! Pistons and the blocks built from several fixed boxes.

use super::{Bounds, Collector};

/// Thickness of a cauldron or hopper wall.
const WALL: f32 = 0.125;

/// Floor of a cauldron.
pub(super) const CAULDRON_FLOOR: Bounds = Bounds::new(0.0, 0.0, 0.0, 1.0, 0.3125, 1.0);
/// Floor of a hopper, which fills its lower five eighths whatever way it faces.
pub(super) const HOPPER_FLOOR: Bounds = Bounds::new(0.0, 0.0, 0.0, 1.0, 0.625, 1.0);

/// A cauldron or hopper: the floor, then the west, north, east and south walls.
pub(super) fn basin(boxes: &mut Collector<'_>, floor: Bounds) {
    boxes.local(floor);
    boxes.local(Bounds::new(0.0, 0.0, 0.0, WALL, 1.0, 1.0));
    boxes.local(Bounds::new(0.0, 0.0, 0.0, 1.0, 1.0, WALL));
    boxes.local(Bounds::new(1.0 - WALL, 0.0, 0.0, 1.0, 1.0, 1.0));
    boxes.local(Bounds::new(0.0, 0.0, 1.0 - WALL, 1.0, 1.0, 1.0));
}

/// A brewing stand: the centre rod, then the base plate.
pub(super) fn brewing_stand(boxes: &mut Collector<'_>) {
    boxes.local(Bounds::new(0.4375, 0.0, 0.4375, 0.5625, 0.875, 0.5625));
    boxes.local(Bounds::new(0.0, 0.0, 0.0, 1.0, 0.125, 1.0));
}

/// An end portal frame: the frame, then the eye on top when metadata bit 4 is set.
pub(super) fn end_portal_frame(boxes: &mut Collector<'_>, meta: i32) {
    boxes.local(Bounds::new(0.0, 0.0, 0.0, 1.0, 0.8125, 1.0));
    if meta & 4 != 0 {
        boxes.local(Bounds::new(0.3125, 0.8125, 0.3125, 0.6875, 1.0, 0.6875));
    }
}

/// A piston base: the unit cube, or three quarters of it on the side away from the head when
/// extended (metadata bit 8). An extended base with an orientation above 5 stays a unit cube.
pub(super) fn piston_base(meta: i32) -> Bounds {
    if meta & 8 == 0 {
        return Bounds::FULL;
    }
    match meta & 7 {
        0 => Bounds::new(0.0, 0.25, 0.0, 1.0, 1.0, 1.0),
        1 => Bounds::new(0.0, 0.0, 0.0, 1.0, 0.75, 1.0),
        2 => Bounds::new(0.0, 0.0, 0.25, 1.0, 1.0, 1.0),
        3 => Bounds::new(0.0, 0.0, 0.0, 1.0, 1.0, 0.75),
        4 => Bounds::new(0.25, 0.0, 0.0, 1.0, 1.0, 1.0),
        5 => Bounds::new(0.0, 0.0, 0.0, 0.75, 1.0, 1.0),
        _ => Bounds::FULL,
    }
}

/// A piston head: the quarter-thick plate on the face its low three metadata bits name, then the
/// arm behind it. Orientations above 5 clamp to 5.
///
/// The arm boxes are the game's own, including the west-facing arm that runs along Z.
pub(super) fn piston_head(boxes: &mut Collector<'_>, meta: i32) {
    let (plate, arm) = match (meta & 7).min(5) {
        0 => (
            Bounds::new(0.0, 0.0, 0.0, 1.0, 0.25, 1.0),
            Bounds::new(0.375, 0.25, 0.375, 0.625, 1.0, 0.625),
        ),
        1 => (
            Bounds::new(0.0, 0.75, 0.0, 1.0, 1.0, 1.0),
            Bounds::new(0.375, 0.0, 0.375, 0.625, 0.75, 0.625),
        ),
        2 => (
            Bounds::new(0.0, 0.0, 0.0, 1.0, 1.0, 0.25),
            Bounds::new(0.25, 0.375, 0.25, 0.75, 0.625, 1.0),
        ),
        3 => (
            Bounds::new(0.0, 0.0, 0.75, 1.0, 1.0, 1.0),
            Bounds::new(0.25, 0.375, 0.0, 0.75, 0.625, 0.75),
        ),
        4 => (
            Bounds::new(0.0, 0.0, 0.0, 0.25, 1.0, 1.0),
            Bounds::new(0.375, 0.25, 0.25, 0.625, 0.75, 1.0),
        ),
        _ => (
            Bounds::new(0.75, 0.0, 0.0, 1.0, 1.0, 1.0),
            Bounds::new(0.0, 0.375, 0.25, 0.75, 0.625, 0.75),
        ),
    };
    boxes.local(plate);
    boxes.local(arm);
}
