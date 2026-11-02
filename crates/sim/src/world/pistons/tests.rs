use super::PistonPush;
use crate::{Aabb, Vec3};

/// A unit cube with its least corner at `min`.
fn cube(min: [f64; 3]) -> Aabb {
    let min = Vec3::new(min[0], min[1], min[2]);
    Aabb::new(min, min + Vec3::ONE)
}

#[test]
fn a_push_carries_the_trailing_face_to_the_cube_s_leading_face() {
    let mover = Aabb::new(Vec3::new(5.0, 4.0, 4.25), Vec3::new(5.5, 5.75, 4.75));
    let east = PistonPush {
        cube: cube([4.5, 4.0, 4.0]),
        direction: [1, 0, 0],
    };
    assert_eq!(east.delta(mover), Some(Vec3::new(0.5, 0.0, 0.0)));
    let down = PistonPush {
        cube: cube([5.0, 5.5, 4.0]),
        direction: [0, -1, 0],
    };
    assert_eq!(down.delta(mover), Some(Vec3::new(0.0, -0.25, 0.0)));
}

#[test]
fn a_cube_that_only_touches_the_mover_pushes_nothing() {
    let mover = Aabb::new(Vec3::new(5.0, 4.0, 4.2), Vec3::new(5.6, 5.8, 4.8));
    let push = PistonPush {
        cube: cube([4.0, 4.0, 4.0]),
        direction: [1, 0, 0],
    };
    assert_eq!(push.delta(mover), None);
}
