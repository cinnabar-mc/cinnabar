//! CPU sampling of the panorama pass, for window-free screenshots of screens drawn over it.

use super::{PanoramaFaces, PanoramaView};

/// The tinted linear-light colour the panorama pass shows at pixel `(x, y)` of a `size` viewport.
///
/// Samples the pixel centre with the shader's cube convention and linear filtering, then
/// composites the view's straight-alpha tint, as `panorama.wgsl` does before sRGB encoding.
#[must_use]
pub fn background(
    faces: &PanoramaFaces,
    view: &PanoramaView,
    size: [u32; 2],
    x: u32,
    y: u32,
) -> [f32; 3] {
    let [yaw, pitch, tan_half, aspect, ..] = view.shader_uniform();
    let (sp, cp) = pitch.sin_cos();
    let (sy, cy) = yaw.sin_cos();
    let (x, y) = (x as f32 + 0.5, y as f32 + 0.5);
    let dx = (x / size[0] as f32 * 2.0 - 1.0) * tan_half * aspect;
    let dy = (1.0 - y / size[1] as f32 * 2.0) * tan_half;
    let pitched_y = dy * cp + sp;
    let pitched_z = dy * sp - cp;
    let direction = [
        dx * cy - pitched_z * sy,
        pitched_y,
        dx * sy + pitched_z * cy,
    ];
    let (face, uv) = face_uv(direction);
    let sampled = sample(faces, face, uv);
    std::array::from_fn(|channel| {
        sampled[channel] * (1.0 - view.tint[3]) + view.tint[channel] * view.tint[3]
    })
}

/// Selects the pack face seen from inside the cube and its normalized coordinates.
fn face_uv([x, y, z]: [f32; 3]) -> (usize, [f32; 2]) {
    let [ax, ay, az] = [x.abs(), y.abs(), z.abs()];
    let (face, u, v) = if ay >= ax && ay >= az {
        if y > 0.0 {
            (4, x / ay, -z / ay)
        } else {
            (5, x / ay, z / ay)
        }
    } else if ax >= az {
        if x > 0.0 {
            (1, z / ax, -y / ax)
        } else {
            (3, -z / ax, -y / ax)
        }
    } else if z < 0.0 {
        (0, x / az, -y / az)
    } else {
        (2, -x / az, -y / az)
    };
    (face, [u * 0.5 + 0.5, v * 0.5 + 0.5])
}

/// Bilinearly samples one sRGB face in linear color, clamping at its edges.
fn sample(faces: &PanoramaFaces, face: usize, uv: [f32; 2]) -> [f32; 3] {
    let side = faces.side() as usize;
    let [x, y] =
        uv.map(|coordinate| (coordinate * side as f32 - 0.5).clamp(0.0, (side - 1) as f32));
    let [left, top] = [x as usize, y as usize];
    let [right, bottom] = [(left + 1).min(side - 1), (top + 1).min(side - 1)];
    let [fx, fy] = [x - left as f32, y - top as f32];
    let texel = |x, y, channel| {
        let index = ((face * side + y) * side + x) * 4 + channel;
        srgb_to_linear(faces.layer_major()[index])
    };
    std::array::from_fn(|channel| {
        let a = texel(left, top, channel) * (1.0 - fx) + texel(right, top, channel) * fx;
        let b = texel(left, bottom, channel) * (1.0 - fx) + texel(right, bottom, channel) * fx;
        a * (1.0 - fy) + b * fy
    })
}

/// Decodes an sRGB channel to linear light, as the GPU's sRGB texture formats do.
#[must_use]
pub fn srgb_to_linear(value: u8) -> f32 {
    let value = f32::from(value) / 255.0;
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

/// Encodes linear light as an sRGB channel in `0.0..=255.0`, as an sRGB render target does.
#[must_use]
pub fn linear_to_srgb(value: f32) -> f32 {
    let value = value.clamp(0.0, 1.0);
    255.0
        * if value <= 0.0031308 {
            value * 12.92
        } else {
            1.055 * value.powf(1.0 / 2.4) - 0.055
        }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panorama::launcher_view;

    #[test]
    fn untinted_uniform_faces_sample_their_own_colour() {
        let faces =
            PanoramaFaces::new(1, std::array::from_fn(|_| vec![120, 160, 200, 255])).unwrap();
        let view = launcher_view(0.0, 2.0, [0.0; 4]);
        let colour = background(&faces, &view, [2, 1], 0, 0).map(|c| linear_to_srgb(c).round());
        assert_eq!(colour, [120.0, 160.0, 200.0]);
        let tinted = launcher_view(0.0, 2.0, [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(background(&faces, &tinted, [2, 1], 1, 0), [0.0; 3]);
    }

    #[test]
    fn cube_axes_select_the_pack_faces_in_order() {
        for (face, direction) in [
            [0.0, 0.0, -1.0],
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [-1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, -1.0, 0.0],
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(face_uv(direction), (face, [0.5, 0.5]));
        }
    }
}
