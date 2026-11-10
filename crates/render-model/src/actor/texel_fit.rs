//! The power-of-two box reduction every bounded actor-art allocation applies past its limit.

/// Box-filters whole RGBA8 layers by powers of two until their longest side fits limit.
/// Averages partial edge blocks; returns None for zero sizes/limit or artwork that already fits.
pub fn fit_rgba_within(
    width: u16,
    height: u16,
    rgba8: &[u8],
    limit: u32,
) -> Option<([u16; 2], Vec<u8>)> {
    let longest = u32::from(width.max(height));
    let layer = usize::from(width) * usize::from(height) * 4;
    if longest <= limit || limit == 0 || layer == 0 {
        return None;
    }
    let factor = reduction(width, height, limit);
    let [out_width, out_height] = fitted_dimensions(width, height, limit).map(usize::from);
    let mut out = Vec::with_capacity(out_width * out_height * 4 * (rgba8.len() / layer));
    for layer in rgba8.chunks_exact(layer) {
        for y in 0..out_height {
            for x in 0..out_width {
                out.extend(block_mean(
                    width,
                    height,
                    factor,
                    x,
                    y,
                    |source_x, source_y| {
                        let at = (source_y * usize::from(width) + source_x) * 4;
                        [layer[at], layer[at + 1], layer[at + 2], layer[at + 3]]
                    },
                ));
            }
        }
    }
    Some(([out_width as u16, out_height as u16], out))
}

/// One channel of the texel at `[x, y]` of [`fitted_dimensions`] that `fit_rgba_within` would
/// produce, reading source texels through `sample`; artwork that fits is sampled unchanged.
/// Lets callers measure a reduced image without building the source or the reduction.
pub fn fitted_channel(
    width: u16,
    height: u16,
    limit: u32,
    x: usize,
    y: usize,
    sample: impl Fn(usize, usize) -> u8,
) -> u8 {
    let [channel] = block_mean(
        width,
        height,
        reduction(width, height, limit),
        x,
        y,
        |x, y| [sample(x, y)],
    );
    channel
}

/// The dimensions `fit_rgba_within` produces for `limit`; unchanged when they already fit.
pub fn fitted_dimensions(width: u16, height: u16, limit: u32) -> [u16; 2] {
    let factor = reduction(width, height, limit) as u32;
    [width, height].map(|side| u32::from(side).div_ceil(factor) as u16)
}

/// The power-of-two box size for `limit`, or 1 when the artwork already fits.
pub(super) fn reduction(width: u16, height: u16, limit: u32) -> usize {
    let longest = u32::from(width.max(height));
    if longest <= limit || limit == 0 {
        return 1;
    }
    longest.div_ceil(limit).next_power_of_two() as usize
}

/// The per-channel truncated mean of `sample` over output texel `[x, y]`'s box, clipped at the
/// edges; zero for a texel outside the reduced image.
fn block_mean<const N: usize>(
    width: u16,
    height: u16,
    factor: usize,
    x: usize,
    y: usize,
    sample: impl Fn(usize, usize) -> [u8; N],
) -> [u8; N] {
    let (mut sum, mut count) = ([0u32; N], 0u32);
    for source_y in y * factor..((y + 1) * factor).min(usize::from(height)) {
        for source_x in x * factor..((x + 1) * factor).min(usize::from(width)) {
            for (total, value) in sum.iter_mut().zip(sample(source_x, source_y)) {
                *total += u32::from(value);
            }
            count += 1;
        }
    }
    sum.map(|total| total.checked_div(count).unwrap_or(0) as u8)
}

#[cfg(test)]
mod tests {
    use super::{fit_rgba_within, fitted_channel, fitted_dimensions};

    #[test]
    fn oversized_art_halves_by_powers_of_two_and_averages_odd_edges() {
        let pixels = (0..3u8 * 2)
            .flat_map(|texel| [texel * 10, 0, 0, 255])
            .collect::<Vec<_>>();
        let ([width, height], reduced) = fit_rgba_within(3, 2, &pixels, 2).unwrap();
        assert_eq!([width, height], [2, 1]);
        // Reds 0, 10, 30 and 40 average to 20; the odd column's 20 and 50 average to 35.
        assert_eq!(reduced, [20, 0, 0, 255, 35, 0, 0, 255]);
        let ([width, height], _) =
            fit_rgba_within(1024, 1024, &vec![9; 1024 * 1024 * 4], 512).unwrap();
        assert_eq!([width, height], [512, 512]);
        let ([width, height], _) = fit_rgba_within(654, 576, &vec![9; 654 * 576 * 4], 512).unwrap();
        assert_eq!([width, height], [327, 288]);
    }

    #[test]
    fn fitting_or_empty_art_is_left_alone() {
        assert!(fit_rgba_within(512, 512, &vec![0; 512 * 512 * 4], 512).is_none());
        assert!(fit_rgba_within(4, 4, &[0; 64], 0).is_none());
        assert!(fit_rgba_within(0, 4, &[], 2).is_none());
    }

    #[test]
    fn fitted_channels_read_the_reduced_texels_in_place() {
        let pixels = (0..5u8 * 3)
            .flat_map(|texel| [texel * 7, texel, 255 - texel, texel * 3])
            .collect::<Vec<_>>();
        for limit in [2, 3, 5] {
            let [width, height] = fitted_dimensions(5, 3, limit);
            let reduced = fit_rgba_within(5, 3, &pixels, limit)
                .map_or(pixels.clone(), |(_, reduced)| reduced);
            for y in 0..usize::from(height) {
                for x in 0..usize::from(width) {
                    for channel in 0..4 {
                        let read = fitted_channel(5, 3, limit, x, y, |sx, sy| {
                            pixels[(sy * 5 + sx) * 4 + channel]
                        });
                        let at = (y * usize::from(width) + x) * 4 + channel;
                        assert_eq!(read, reduced[at], "limit {limit}, texel {x},{y}");
                    }
                }
            }
        }
    }
}
