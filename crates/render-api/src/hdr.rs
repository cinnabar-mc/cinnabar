//! HDR display output: the player's request and the signal encodings shared by the renderer
//! and frame captures.
//!
//! Colour before encoding is linear BT.709 where `1.0` is paper white, the level of SDR white,
//! menus and the HUD. HDR highlights exceed `1.0` up to [`HdrOutput::headroom`].

/// Default brightest level highlights reach, in nits.
pub const DEFAULT_HDR_PEAK_NITS: u16 = 1000;
/// Peak brightness bounds offered to the player, in nits.
pub const HDR_PEAK_NITS: (u16, u16) = (400, 4000);
/// Default SDR white level, in nits.
pub const DEFAULT_HDR_PAPER_WHITE_NITS: u16 = 200;
/// Paper-white bounds offered to the player, in nits.
pub const HDR_PAPER_WHITE_NITS: (u16, u16) = (80, 500);
/// Default share of the headroom SDR-tonemapped scenes expand into, in percent.
pub const DEFAULT_HDR_HIGHLIGHT_BOOST_PERCENT: u8 = 50;
/// Luminance where SDR-scene highlight expansion begins; darker colour is unchanged.
pub const HDR_EXPANSION_KNEE: f32 = 0.5;

/// The luminance scRGB assigns to `1.0`.
pub const SCRGB_REFERENCE_NITS: f32 = 80.0;
/// The luminance PQ assigns to its full-scale signal.
pub const PQ_PEAK_NITS: f32 = 10_000.0;

/// The player's HDR output request.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HdrOutput {
    /// Present through an HDR swapchain when the display path offers one.
    pub enabled: bool,
    /// Brightest level highlights may reach, in nits.
    pub peak_nits: f32,
    /// Level of SDR white, menus and the HUD, in nits.
    pub paper_white_nits: f32,
    /// Share of the headroom that SDR-tonemapped scenes expand into, `0..=1`.
    pub highlight_boost: f32,
}

impl Default for HdrOutput {
    fn default() -> Self {
        Self {
            enabled: false,
            peak_nits: f32::from(DEFAULT_HDR_PEAK_NITS),
            paper_white_nits: f32::from(DEFAULT_HDR_PAPER_WHITE_NITS),
            highlight_boost: f32::from(DEFAULT_HDR_HIGHLIGHT_BOOST_PERCENT) / 100.0,
        }
    }
}

impl HdrOutput {
    /// How far above paper white highlights may go, as a linear multiple; at least `1.0`.
    #[must_use]
    pub fn headroom(self) -> f32 {
        (self.peak_nits / self.paper_white_nits.max(1.0)).max(1.0)
    }

    /// The level SDR-scene white expands to, as a multiple of paper white.
    #[must_use]
    pub fn sdr_white(self) -> f32 {
        1.0 + (self.headroom() - 1.0) * self.highlight_boost.clamp(0.0, 1.0)
    }
}

/// How the swapchain carries HDR colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HdrEncoding {
    /// Linear BT.709 in half floats, `1.0` = [`SCRGB_REFERENCE_NITS`].
    Scrgb,
    /// BT.2020 primaries with the SMPTE ST 2084 transfer, in 10-bit unorm.
    Pq,
}

impl HdrEncoding {
    /// Recovers linear BT.709 colour relative to paper white from one encoded texel.
    #[must_use]
    pub fn decode(self, texel: [f32; 3], paper_white_nits: f32) -> [f32; 3] {
        let paper_white = paper_white_nits.max(1.0);
        match self {
            Self::Scrgb => texel.map(|value| value * SCRGB_REFERENCE_NITS / paper_white),
            Self::Pq => {
                let nits = texel.map(|value| pq_eotf(value) * PQ_PEAK_NITS / paper_white);
                mul(BT2020_TO_BT709, nits)
            }
        }
    }
}

/// Encodes linear colour relative to paper white as an 8-bit sRGB SDR pixel, clipping highlights.
#[must_use]
pub fn sdr_srgb8(linear: [f32; 3]) -> [u8; 3] {
    linear.map(|value| {
        let value = value.clamp(0.0, 1.0);
        let encoded = if value <= 0.003_130_8 {
            value * 12.92
        } else {
            1.055 * value.powf(1.0 / 2.4) - 0.055
        };
        (encoded * 255.0 + 0.5) as u8
    })
}

/// Converts linear BT.2020 to linear BT.709 (ITU-R BT.2087).
const BT2020_TO_BT709: [[f32; 3]; 3] = [
    [1.660_491, -0.587_641_1, -0.072_849_86],
    [-0.124_550_5, 1.132_9, -0.008_349_42],
    [-0.018_150_76, -0.100_578_9, 1.118_729_7],
];

fn mul(matrix: [[f32; 3]; 3], value: [f32; 3]) -> [f32; 3] {
    matrix.map(|row| row[0] * value[0] + row[1] * value[1] + row[2] * value[2])
}

/// SMPTE ST 2084 EOTF: a PQ signal in `0..=1` to luminance relative to [`PQ_PEAK_NITS`].
fn pq_eotf(signal: f32) -> f32 {
    const M1: f32 = 2610.0 / 16384.0;
    const M2: f32 = 2523.0 / 4096.0 * 128.0;
    const C1: f32 = 3424.0 / 4096.0;
    const C2: f32 = 2413.0 / 4096.0 * 32.0;
    const C3: f32 = 2392.0 / 4096.0 * 32.0;
    let power = signal.clamp(0.0, 1.0).powf(1.0 / M2);
    ((power - C1).max(0.0) / (C2 - C3 * power)).powf(1.0 / M1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headroom_is_peak_over_paper_white_and_never_below_sdr() {
        let output = HdrOutput {
            peak_nits: 1000.0,
            paper_white_nits: 200.0,
            ..HdrOutput::default()
        };
        assert_eq!(output.headroom(), 5.0);
        let dim = HdrOutput {
            peak_nits: 100.0,
            ..output
        };
        assert_eq!(dim.headroom(), 1.0);
    }

    #[test]
    fn highlight_boost_scales_sdr_white_between_paper_white_and_peak() {
        let output = HdrOutput {
            peak_nits: 1000.0,
            paper_white_nits: 200.0,
            highlight_boost: 0.0,
            ..HdrOutput::default()
        };
        assert_eq!(output.sdr_white(), 1.0);
        assert_eq!(
            HdrOutput {
                highlight_boost: 0.5,
                ..output
            }
            .sdr_white(),
            3.0
        );
        assert_eq!(
            HdrOutput {
                highlight_boost: 2.0,
                ..output
            }
            .sdr_white(),
            5.0
        );
    }

    #[test]
    fn scrgb_paper_white_decodes_to_one() {
        let decoded = HdrEncoding::Scrgb.decode([2.5, 2.5, 2.5], 200.0);
        assert_eq!(decoded, [1.0, 1.0, 1.0]);
    }

    #[test]
    fn pq_reference_levels_decode_to_their_luminance() {
        // 0.5081 is the PQ code value of 100 nits; 0.7518 of 1000 nits.
        let [r, g, b] = HdrEncoding::Pq.decode([0.508_078, 0.508_078, 0.508_078], 100.0);
        for channel in [r, g, b] {
            assert!((channel - 1.0).abs() < 1.0e-3, "{channel}");
        }
        let [r, ..] = HdrEncoding::Pq.decode([0.751_827, 0.751_827, 0.751_827], 100.0);
        assert!((r - 10.0).abs() < 1.0e-2, "{r}");
        assert_eq!(HdrEncoding::Pq.decode([0.0; 3], 200.0), [0.0; 3]);
    }

    #[test]
    fn sdr_capture_clips_highlights_and_encodes_srgb() {
        assert_eq!(sdr_srgb8([0.0, 1.0, 4.0]), [0, 255, 255]);
        assert_eq!(sdr_srgb8([0.215_861, 0.5, -1.0]), [128, 188, 0]);
    }
}
