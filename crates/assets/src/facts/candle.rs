//! Native candle placement inside a block cell, shared by candle geometry and flames.

/// Pixels in one block, the unit of candle heights.
const PIXELS_PER_BLOCK: f32 = 16.0;

/// Where a lit candle cake's flame callback starts, in block units.
const CAKE_FLAME_ORIGIN: [f32; 3] = [0.5, 0.85, 0.5];

/// The native candles of one zero-based candle count, in drawing and flame order: each candle's
/// height in pixels and its horizontal `[x, z]` centre in blocks.
#[must_use]
pub const fn placed_candles(count: u8) -> &'static [(u16, [f32; 2])] {
    match count {
        0 => &[(6, [0.5, 0.5])],
        1 => &[(6, [0.625, 0.5]), (5, [0.375, 0.5625])],
        2 => &[(6, [0.5625, 0.4375]), (5, [0.375, 0.5]), (3, [0.5, 0.625])],
        _ => &[
            (6, [0.5625, 0.375]),
            (5, [0.375, 0.375]),
            (5, [0.625, 0.5625]),
            (3, [0.4375, 0.5625]),
        ],
    }
}

/// The flames one block state shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CandleFlames {
    /// Unlit, or not a candle block.
    #[default]
    None,
    /// A lit standalone candle block with this zero-based candle count.
    Candles(u8),
    /// A lit candle cake.
    Cake,
}

impl CandleFlames {
    /// Each candle's flame callback origin relative to the block cell, in native order.
    pub fn origins(self) -> impl Iterator<Item = [f32; 3]> {
        let candles = match self {
            Self::Candles(count) => placed_candles(count),
            Self::None | Self::Cake => &[],
        };
        candles
            .iter()
            .map(|&(height, [x, z])| [x, f32::from(height) / PIXELS_PER_BLOCK, z])
            .chain((self == Self::Cake).then_some(CAKE_FLAME_ORIGIN))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flame_origins_follow_the_native_candle_layouts() {
        let origins = |flames: CandleFlames| flames.origins().collect::<Vec<_>>();
        assert_eq!(origins(CandleFlames::None), [[0.0; 3]; 0]);
        assert_eq!(origins(CandleFlames::Candles(0)), [[0.5, 0.375, 0.5]]);
        assert_eq!(
            origins(CandleFlames::Candles(1)),
            [[0.625, 0.375, 0.5], [0.375, 0.3125, 0.5625]]
        );
        assert_eq!(
            origins(CandleFlames::Candles(2)),
            [
                [0.5625, 0.375, 0.4375],
                [0.375, 0.3125, 0.5],
                [0.5, 0.1875, 0.625]
            ]
        );
        assert_eq!(
            origins(CandleFlames::Candles(3)),
            [
                [0.5625, 0.375, 0.375],
                [0.375, 0.3125, 0.375],
                [0.625, 0.3125, 0.5625],
                [0.4375, 0.1875, 0.5625]
            ]
        );
        assert_eq!(origins(CandleFlames::Cake), [[0.5, 0.85, 0.5]]);
    }
}
