//! Switch full-input payloads, excluding the report ID supplied separately by HID.
//!
//! Layout reference: dekuNukem/Nintendo_Switch_Reverse_Engineering,
//! bluetooth_hid_notes.md, "Standard input report format". No reference code copied.

pub(super) const VENDOR_ID: u16 = 0x057e;
pub(super) const PRODUCT_ID: u16 = 0x2009;
pub(super) const REPORT_ID: u32 = 0x30;
pub(super) const PAYLOAD_LENGTH: usize = 48;

/// State carried by full reports; timer, battery, vibration and IMU are ignored.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct State {
    pub buttons: u32,
    pub axes: [f32; 4],
}

impl State {
    /// Decode only the full report layout witnessed on the Nova Lite in Switch mode.
    pub fn decode(report_id: u32, payload: &[u8]) -> Option<Self> {
        if report_id != REPORT_ID || payload.len() != PAYLOAD_LENGTH {
            return None;
        }
        let stick = |offset: usize| {
            let horizontal =
                u16::from(payload[offset]) | (u16::from(payload[offset + 1] & 0x0f) << 8);
            let vertical =
                u16::from(payload[offset + 1] >> 4) | (u16::from(payload[offset + 2]) << 4);
            [normalize(horizontal), normalize(vertical)]
        };
        let left = stick(5);
        let right = stick(8);
        Some(Self {
            buttons: u32::from_le_bytes([payload[2], payload[3], payload[4], 0]),
            axes: [left[0], left[1], right[0], right[1]],
        })
    }
}

/// Nominal Nova Lite 12-bit range. Factory/user SPI calibration is not inferred.
fn normalize(value: u16) -> f32 {
    let centered = f32::from(value) - 2048.0;
    centered / if centered < 0.0 { 2048.0 } else { 2047.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn centered() -> [u8; PAYLOAD_LENGTH] {
        let mut payload = [0; PAYLOAD_LENGTH];
        payload[5..11].copy_from_slice(&[0, 8, 128, 0, 8, 128]);
        payload
    }

    #[test]
    fn captured_neutral_report_has_centered_sticks() {
        let state = State::decode(REPORT_ID, &centered()).unwrap();
        assert_eq!(state.buttons, 0);
        assert_eq!(state.axes, [0.0; 4]);
    }

    #[test]
    fn packed_sticks_keep_all_twelve_bits_and_bevy_positive_up() {
        let mut payload = centered();
        payload[5..11].copy_from_slice(&[0xff, 0x0f, 0, 0, 0xf0, 0xff]);
        assert_eq!(
            State::decode(REPORT_ID, &payload).unwrap().axes,
            [1.0, -1.0, -1.0, 1.0]
        );
        payload[5..8].copy_from_slice(&[0x23, 0x61, 0x45]);
        let state = State::decode(REPORT_ID, &payload).unwrap();
        assert_eq!(state.axes[0], normalize(0x123));
        assert_eq!(state.axes[1], normalize(0x456));
    }

    #[test]
    fn every_button_bit_survives_without_timer_or_battery_bits() {
        for bit in 0..24 {
            let mut payload = centered();
            payload[2 + bit / 8] = 1 << (bit % 8);
            payload[0] = 0xff;
            payload[1] = 0xff;
            assert_eq!(
                State::decode(REPORT_ID, &payload).unwrap().buttons,
                1 << bit
            );
        }
    }

    #[test]
    fn unsupported_or_malformed_reports_are_not_controls() {
        for length in 0..PAYLOAD_LENGTH {
            assert!(State::decode(REPORT_ID, &centered()[..length]).is_none());
        }
        assert!(State::decode(0x21, &centered()).is_none());
        assert!(State::decode(0x3f, &centered()).is_none());
        assert!(State::decode(REPORT_ID, &[0; PAYLOAD_LENGTH + 1]).is_none());
    }
}
