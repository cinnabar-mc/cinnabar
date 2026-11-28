//! UV flags that turn a cube face's art with its block's quarter-turn transformation.

/// World directions in which a cube face's native U and V grow, per `greedy_uv` in
/// `chunk.wesl`: West +Z/-Y, East -Z/-Y, Down +X/-Z, Up +X/+Z, North -X/-Y, South +X/-Y.
const CUBE_UV_AXES: [[[i32; 3]; 2]; 6] = [
    [[0, 0, 1], [0, -1, 0]],
    [[0, 0, -1], [0, -1, 0]],
    [[1, 0, 0], [0, 0, -1]],
    [[1, 0, 0], [0, 0, 1]],
    [[-1, 0, 0], [0, -1, 0]],
    [[1, 0, 0], [0, -1, 0]],
];

/// UV flags (quarter-turn code and reflections) that draw model face `source`'s art on world
/// face `face` of a cube turned by `rotation`, so the art turns with the block instead of
/// keeping the world orientation of an unturned cube. An unturned cube needs none.
pub(super) fn turned_face_uv_flags(source: usize, face: usize, rotation: [i32; 3]) -> u32 {
    if rotation.iter().all(|turns| turns.rem_euclid(4) == 0) {
        return 0;
    }
    let turn = |mut axis_vector: [i32; 3]| {
        for (axis, turns) in rotation.into_iter().enumerate() {
            for _ in 0..turns.rem_euclid(4) {
                axis_vector = block_transform::rotation::rotate_quarter(axis_vector, axis);
            }
        }
        axis_vector
    };
    let [source_u, source_v] = CUBE_UV_AXES[source];
    let wanted = [turn(source_u), turn(source_v)];
    let [face_u, face_v] = CUBE_UV_AXES[face];
    // Texel coordinates `greedy_uv` samples at native face position (u, v) of a unit quad.
    let sample = |u: i32, v: i32, flags: u32| {
        let [mut s, mut t] = match flags & assets::MATERIAL_FLAG_UV_ROTATION_MASK {
            1 => [v, 1 - u],
            2 => [1 - u, 1 - v],
            3 => [1 - v, u],
            _ => [u, v],
        };
        if flags & 4 != 0 {
            s = 1 - s;
        }
        if flags & 8 != 0 {
            t = 1 - t;
        }
        [s, t]
    };
    (0..=assets::MATERIAL_FLAG_UV_MASK)
        .find(|&flags| {
            let origin = sample(0, 0, flags);
            let along_u = sample(1, 0, flags);
            let along_v = sample(0, 1, flags);
            (0..2).all(|texel_axis| {
                let du = along_u[texel_axis] - origin[texel_axis];
                let dv = along_v[texel_axis] - origin[texel_axis];
                let world: [i32; 3] =
                    std::array::from_fn(|axis| du * face_u[axis] + dv * face_v[axis]);
                world == wanted[texel_axis]
            })
        })
        .unwrap_or(0)
}
