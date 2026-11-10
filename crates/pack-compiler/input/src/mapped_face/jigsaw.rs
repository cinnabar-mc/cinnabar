//! Jigsaw blocks build a local frame from `facing_direction` and `rotation`: the front
//! (authored north) faces the facing direction and the lock (authored up) points at the
//! rotated top. Each world face draws the slot of its local direction, turned so the
//! slot's own top follows the frame.

/// Unit normals in native facing order.
const NORMALS: [[i8; 3]; 6] = [
    [0, -1, 0],
    [0, 1, 0],
    [0, 0, -1],
    [0, 0, 1],
    [-1, 0, 0],
    [1, 0, 0],
];
/// The unrotated top for each facing: north for vertical facings, up otherwise.
const TOP: [usize; 6] = [2, 2, 1, 1, 1, 1];
/// The local direction each authored slot's texture treats as its top.
const SLOT_TOP: [usize; 6] = [2, 2, 1, 1, 2, 2];
/// The flip that turns each world face's texture top toward each world direction.
const TOP_FLIPS: [[u8; 6]; 6] = [
    [0, 0, 3, 0, 2, 1],
    [0, 0, 0, 3, 2, 1],
    [3, 0, 0, 0, 2, 1],
    [3, 0, 0, 0, 2, 1],
    [3, 0, 2, 1, 0, 0],
    [3, 0, 2, 1, 0, 0],
];

/// Slots and flips for a valid native `facing` (0..6) and quarter-turn `rotation` (0..4).
pub(super) fn mapping(facing: usize, rotation: usize) -> ([u8; 6], [u8; 6]) {
    let front = NORMALS[facing];
    let mut top = NORMALS[TOP[facing]];
    for _ in 0..rotation {
        top = cross(top, front);
    }
    // Columns are the world directions of local east, up and south.
    let basis = [cross(front, top), top, front.map(|c| -c)];
    let to_local = |world: [i8; 3]| direction(basis.map(|axis| dot(axis, world)));
    let to_world = |local: [i8; 3]| {
        direction(std::array::from_fn(|row| {
            (0..3)
                .map(|column| basis[column][row] * local[column])
                .sum()
        }))
    };
    let slots: [usize; 6] = std::array::from_fn(|face| to_local(NORMALS[face]));
    (
        slots.map(|slot| slot as u8),
        std::array::from_fn(|face| TOP_FLIPS[face][to_world(NORMALS[SLOT_TOP[slots[face]]])]),
    )
}

fn cross(a: [i8; 3], b: [i8; 3]) -> [i8; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [i8; 3], b: [i8; 3]) -> i8 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}

/// The facing index of an axis-aligned unit vector; the frame keeps every product one.
fn direction(vector: [i8; 3]) -> usize {
    NORMALS
        .iter()
        .position(|normal| *normal == vector)
        .expect("jigsaw frames stay axis-aligned")
}
