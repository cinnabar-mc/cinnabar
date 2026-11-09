//! How a cell overlay, authored with its front out of north (-z) and its top up, turns to the face
//! it is drawn on: AE2's `BlockOrientation` for that face at spin 0, whose top of a side overlay
//! is up and whose top of an up or down overlay is north. The server runtime picks overlays with
//! this table, and the client draws and picks them with it.

use crate::rotation::rotate_turns;

/// Overlays one cell may carry, in slots 0 to `MAX_CELL_OVERLAYS - 1`: a cable bus's arm and part
/// on each of its six sides, or a drive's ten cells.
pub const MAX_CELL_OVERLAYS: usize = 12;
/// The widest an item overlay may be, in pixels: a cell face's edge.
pub const MAX_OVERLAY_ITEM_SIZE: f32 = 16.0;
/// How far an item overlay's centre may be from its face's centre on each axis, in pixels.
pub const MAX_OVERLAY_ITEM_OFFSET: f32 = MAX_OVERLAY_ITEM_SIZE / 2.0;
/// What an overlay's selection box grows by on each side when a click picks it, in pixels: AE2's
/// 0.002 blocks (`parts/CableBusContainer.java:362`).
pub const OVERLAY_PICK_MARGIN: f32 = 0.032;
/// How far in front of its face an item overlay is drawn, in pixels: AE2's 0.01 blocks
/// (`init/client/InitStackRenderHandlers.java`, `drawOnBlockFace`).
pub const OVERLAY_ITEM_LIFT: f32 = 0.16;

/// Quarter turns about x, then about y, as [`rotate_turns`] makes them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FaceTurn {
    pub x: u8,
    pub y: u8,
}

/// The turn to each face, by Bedrock face id: down, up, north, south, west, east.
pub const FACE_TURNS: [FaceTurn; 6] = [
    FaceTurn { x: 3, y: 0 },
    FaceTurn { x: 1, y: 2 },
    FaceTurn { x: 0, y: 0 },
    FaceTurn { x: 0, y: 2 },
    FaceTurn { x: 0, y: 1 },
    FaceTurn { x: 0, y: 3 },
];

impl FaceTurn {
    /// The turn to `face`, a Bedrock face id; none past east (5).
    #[must_use]
    pub const fn of(face: u8) -> Option<Self> {
        if (face as usize) < FACE_TURNS.len() {
            Some(FACE_TURNS[face as usize])
        } else {
            None
        }
    }

    /// Turns the direction `value`.
    #[must_use]
    pub fn turn<T: Copy + std::ops::Neg<Output = T>>(self, value: [T; 3]) -> [T; 3] {
        rotate_turns(value, [self.x, self.y, 0])
    }

    /// Turns `point` about the centre of a cell `side` units wide whose corner is the origin.
    #[must_use]
    pub fn turn_point(self, point: [f32; 3], side: f32) -> [f32; 3] {
        let half = side / 2.0;
        self.turn(point.map(|axis| axis - half))
            .map(|axis| axis + half)
    }

    /// The axis-aligned box from `min` to `max` turned about the centre of a cell `side` units
    /// wide, as its new minimum and maximum corners.
    #[must_use]
    pub fn turn_box(self, min: [f32; 3], max: [f32; 3], side: f32) -> ([f32; 3], [f32; 3]) {
        let a = self.turn_point(min, side);
        let b = self.turn_point(max, side);
        (
            std::array::from_fn(|axis| a[axis].min(b[axis])),
            std::array::from_fn(|axis| a[axis].max(b[axis])),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{FACE_TURNS, FaceTurn};

    /// The images of +x, +y and +z for each face, from the overlay brief's table.
    const IMAGES: [[[i32; 3]; 3]; 6] = [
        [[1, 0, 0], [0, 0, -1], [0, 1, 0]],
        [[-1, 0, 0], [0, 0, -1], [0, -1, 0]],
        [[1, 0, 0], [0, 1, 0], [0, 0, 1]],
        [[-1, 0, 0], [0, 1, 0], [0, 0, -1]],
        [[0, 0, -1], [0, 1, 0], [1, 0, 0]],
        [[0, 0, 1], [0, 1, 0], [-1, 0, 0]],
    ];

    #[test]
    fn each_face_turns_the_axes_as_ae2_orients_it() {
        for (face, images) in IMAGES.iter().enumerate() {
            let turn = FACE_TURNS[face];
            for (axis, image) in images.iter().enumerate() {
                let mut unit = [0; 3];
                unit[axis] = 1;
                assert_eq!(turn.turn(unit), *image, "face {face} axis {axis}");
            }
        }
    }

    /// The front, authored out of north, ends up out of the overlay's face.
    #[test]
    fn the_front_faces_out_of_its_face() {
        let outward = [
            [0, -1, 0],
            [0, 1, 0],
            [0, 0, -1],
            [0, 0, 1],
            [-1, 0, 0],
            [1, 0, 0],
        ];
        for (face, outward) in outward.into_iter().enumerate() {
            assert_eq!(FaceTurn::of(face as u8).unwrap().turn([0, 0, -1]), outward);
        }
        assert_eq!(FaceTurn::of(6), None);
    }

    #[test]
    fn a_plate_against_north_lands_against_each_face() {
        let (min, max) = ([0.0, 0.0, 0.0], [16.0, 16.0, 2.0]);
        let expected = [
            ([0.0, 0.0, 0.0], [16.0, 2.0, 16.0]),
            ([0.0, 14.0, 0.0], [16.0, 16.0, 16.0]),
            ([0.0, 0.0, 0.0], [16.0, 16.0, 2.0]),
            ([0.0, 0.0, 14.0], [16.0, 16.0, 16.0]),
            ([0.0, 0.0, 0.0], [2.0, 16.0, 16.0]),
            ([14.0, 0.0, 0.0], [16.0, 16.0, 16.0]),
        ];
        for (face, want) in expected.into_iter().enumerate() {
            assert_eq!(
                FACE_TURNS[face].turn_box(min, max, 16.0),
                want,
                "face {face}"
            );
        }
    }
}
