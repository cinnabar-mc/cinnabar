//! Placement of a minecart's centred display-block model.

/// Display height used when a cart does not supply the offset metadata, in model pixels.
pub const MINECART_DEFAULT_DISPLAY_OFFSET: i32 = 6;
const PIXELS_PER_BLOCK: f32 = 16.0;
const DISPLAY_SCALE: f32 = 0.75;

/// Places a centred display block relative to the drawn cart's feet. The body placement
/// supplies rotation and actor scale; `offset` moves the block in model pixels along its up axis.
#[must_use]
pub fn minecart_display_block_transform(
    world_from_cart: [[f32; 4]; 3],
    offset: i32,
) -> Option<[[f32; 4]; 3]> {
    let height = offset as f32 / PIXELS_PER_BLOCK;
    let rows: [[f32; 4]; 3] = std::array::from_fn(|row| {
        std::array::from_fn(|column| {
            if column == 3 {
                world_from_cart[row][3] + world_from_cart[row][1] * height
            } else {
                world_from_cart[row][column] * DISPLAY_SCALE
            }
        })
    });
    rows.iter()
        .flatten()
        .all(|value| value.is_finite())
        .then_some(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contents_follow_the_drawn_cart_and_its_scale_once() {
        let body = [
            [0.0, -2.0, 0.0, 10.0],
            [2.0, 0.0, 0.0, 64.0],
            [0.0, 0.0, 2.0, 20.0],
        ];
        let placed = minecart_display_block_transform(body, MINECART_DEFAULT_DISPLAY_OFFSET)
            .expect("finite cart");
        assert_eq!(
            placed,
            [
                [0.0, -1.5, 0.0, 9.25],
                [1.5, 0.0, 0.0, 64.0],
                [0.0, 0.0, 1.5, 20.0]
            ]
        );
        let lowered = minecart_display_block_transform(body, -16).unwrap();
        assert_eq!(lowered[0][3], 12.0);
        assert_eq!(lowered[1][3], 64.0);
        assert!(minecart_display_block_transform(body, i32::MAX).is_some());
    }

    #[test]
    fn nonfinite_body_placements_do_not_reach_the_renderer() {
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut body = [[0.0; 4]; 3];
            body[1][3] = value;
            assert!(minecart_display_block_transform(body, 6).is_none());
        }
    }
}
