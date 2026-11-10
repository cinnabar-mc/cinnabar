//! Frame choice for dial items (compasses and clocks), whose icon is one frame of an
//! animation strip, following the native sprite calculators.

use std::f32::consts::{PI, TAU};

/// Frames in the native compass strip; the calculator wraps its frame index at this count.
pub const COMPASS_FRAMES: u32 = 32;
/// Frames in the native clock strip; the calculator wraps its frame index at this count.
pub const CLOCK_FRAMES: u32 = 64;

/// Frames in the native strip of a dial of `kind`.
#[must_use]
pub const fn strip_frames(kind: assets::icon::dial::DialKind) -> u32 {
    match kind {
        assets::icon::dial::DialKind::Compass | assets::icon::dial::DialKind::RecoveryCompass => {
            COMPASS_FRAMES
        }
        assets::icon::dial::DialKind::Clock => CLOCK_FRAMES,
    }
}

/// The side of each square frame when a `width` by `height` texture is a vertical strip holding
/// a dial's native frames, or `None` for any other texture. `exact` demands precisely the native
/// frame count, as for a strip under a plain item name; an atlas may carry more frames, of which
/// only the native count is shown.
#[must_use]
pub fn strip_frame_side(
    kind: assets::icon::dial::DialKind,
    width: u32,
    height: u32,
    exact: bool,
) -> Option<u32> {
    let frames = strip_frames(kind);
    if width == 0 || !height.is_multiple_of(width) {
        return None;
    }
    let held = height / width;
    (if exact {
        held == frames
    } else {
        held >= frames
    })
    .then_some(width)
}

/// Share of the angle error added to the needle's velocity each update.
const NEEDLE_PULL: f32 = 0.1;
/// Velocity kept after each update.
const NEEDLE_DAMPING: f32 = 0.8;
/// Native degrees-to-radians factor in the compass heading.
const DEGREES_TO_RADIANS: f32 = 0.017_453_294;

/// What a compass reads this update.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CompassReading {
    /// Points at `target` (block x and z) from a holder at `position` (x and z) facing `yaw`
    /// degrees, Bedrock's rotation with 0 toward +z.
    Target {
        target: [i32; 2],
        position: [f32; 2],
        yaw: f32,
    },
    /// Spins toward `random`, a uniform draw in `0..1`, as in a dimension without a target.
    Spin { random: f32 },
}

/// What a clock reads this update.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ClockReading {
    /// The sky's celestial angle, as a fraction of a day.
    Sky { celestial_angle: f32 },
    /// Spins toward `random`, a uniform draw in `0..1`, as in a dimension without a sky.
    Spin { random: f32 },
}

/// Whether `dimension` has a sky that compasses and clocks read; the native calculators
/// spin elsewhere. Of Bedrock's dimensions only the Overworld (0) is natural.
#[must_use]
pub const fn natural_dimension(dimension: i32) -> bool {
    dimension == 0
}

/// A needle that eases toward each reading with the native velocity and damping.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DialNeedle {
    angle: f32,
    velocity: f32,
    frame: u32,
}

impl DialNeedle {
    /// The strip frame the needle shows.
    #[must_use]
    pub const fn frame(&self) -> u32 {
        self.frame
    }

    /// Advances a compass needle one update; `instant` jumps straight to the reading.
    pub fn update_compass(&mut self, reading: CompassReading, instant: bool) -> u32 {
        let target = match reading {
            CompassReading::Target {
                target,
                position,
                yaw,
            } => {
                (target[1] as f32 - position[1]).atan2(target[0] as f32 - position[0])
                    - (((yaw as i32) % 360) as f32 - 90.0) * DEGREES_TO_RADIANS
            }
            CompassReading::Spin { random } => random * TAU,
        };
        self.ease(target, instant, |error| {
            let wrapped = (error + PI) % TAU;
            (if wrapped < 0.0 {
                wrapped + TAU
            } else {
                wrapped
            }) - PI
        });
        self.frame = wrap_frame(
            self.angle * (COMPASS_FRAMES as f32 / TAU) + COMPASS_FRAMES as f32,
            COMPASS_FRAMES,
        );
        self.frame
    }

    /// Advances a clock needle one update; `instant` jumps straight to the reading.
    pub fn update_clock(&mut self, reading: ClockReading, instant: bool) -> u32 {
        let target = match reading {
            ClockReading::Sky { celestial_angle } => celestial_angle,
            // The native calculator spins in radians even though the sky reading is a fraction.
            ClockReading::Spin { random } => random * TAU,
        };
        self.ease(target, instant, |mut error| {
            while error < -0.5 {
                error += 1.0;
            }
            while error >= 0.5 {
                error -= 1.0;
            }
            error
        });
        self.frame = wrap_frame((self.angle + 1.0) * CLOCK_FRAMES as f32, CLOCK_FRAMES);
        self.frame
    }

    /// Moves the angle toward `target` through the native velocity filter, using `wrap` to
    /// take the shorter way round.
    fn ease(&mut self, target: f32, instant: bool, wrap: impl FnOnce(f32) -> f32) {
        if !target.is_finite() {
            return;
        }
        if instant {
            self.angle = target;
            self.velocity = 0.0;
            return;
        }
        let error = wrap(target - self.angle).clamp(-1.0, 1.0);
        self.velocity = (error * NEEDLE_PULL + self.velocity) * NEEDLE_DAMPING;
        self.angle += self.velocity;
    }
}

/// The strip frame each dial kind shows this frame; `None` keeps that dial's static icon.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DialFrames {
    pub compass: Option<u32>,
    pub recovery_compass: Option<u32>,
    pub clock: Option<u32>,
}

impl DialFrames {
    /// The frame dials of `kind` show.
    #[must_use]
    pub const fn frame(&self, kind: assets::icon::dial::DialKind) -> Option<u32> {
        match kind {
            assets::icon::dial::DialKind::Compass => self.compass,
            assets::icon::dial::DialKind::RecoveryCompass => self.recovery_compass,
            assets::icon::dial::DialKind::Clock => self.clock,
        }
    }

    /// The carried icon identity dial item `identifier` shows: its strip's frames identifier
    /// and frame. `None` for other items, dials without a frame, or frames `catalog` lacks.
    #[must_use]
    pub fn icon_key(
        &self,
        identifier: &str,
        catalog: &assets::RuntimeIconCatalog,
    ) -> Option<(&'static str, u32)> {
        let strip = assets::icon::dial::dial_strip(identifier)?;
        let frame = self.frame(strip.kind)?;
        (frame < catalog.frame_count(strip.frames_identifier))
            .then_some((strip.frames_identifier, frame))
    }
}

/// Truncates `position` to a frame index as the native calculators do, wrapped into `frames`.
fn wrap_frame(position: f32, frames: u32) -> u32 {
    (position as i32).rem_euclid(frames as i32) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A compass reading toward `target` from the origin facing `yaw`.
    fn toward(target: [i32; 2], yaw: f32) -> CompassReading {
        CompassReading::Target {
            target,
            position: [0.0, 0.0],
            yaw,
        }
    }

    #[test]
    fn compass_frames_follow_the_native_heading_formula() {
        // Facing +z (yaw 0) at a target straight ahead: atan2(10, 0) + 90 degrees = pi.
        let mut needle = DialNeedle::default();
        assert_eq!(needle.update_compass(toward([0, 10], 0.0), true), 16);
        // A target to the east (+x) is a quarter turn back from straight ahead.
        assert_eq!(needle.update_compass(toward([10, 0], 0.0), true), 8);
        // Turning to face west (yaw 90) by a quarter turn moves the needle a quarter strip.
        assert_eq!(needle.update_compass(toward([0, 10], 90.0), true), 8);
        // Yaw wraps by whole turns.
        assert_eq!(needle.update_compass(toward([0, 10], 360.0), true), 16);
    }

    #[test]
    fn compass_needles_ease_with_native_velocity_and_take_the_short_way() {
        let mut needle = DialNeedle::default();
        needle.update_compass(toward([0, 10], 0.0), true);
        let start = needle.angle;
        // A reading just across the wrap is approached the short way, not the long way round.
        let reading = CompassReading::Spin {
            random: (start - 0.5 + TAU) % TAU / TAU,
        };
        needle.update_compass(reading, false);
        assert!((needle.velocity - (-0.5 * NEEDLE_PULL * NEEDLE_DAMPING)).abs() < 1e-5);
        assert!((needle.angle - (start + needle.velocity)).abs() < 1e-5);
        // Errors beyond one radian pull no harder than one radian.
        let mut far = DialNeedle::default();
        far.update_compass(CompassReading::Spin { random: 0.4 }, false);
        assert!((far.velocity - NEEDLE_PULL * NEEDLE_DAMPING).abs() < 1e-6);
    }

    #[test]
    fn clock_frames_follow_the_celestial_angle() {
        let mut needle = DialNeedle::default();
        let sky = |celestial_angle| ClockReading::Sky { celestial_angle };
        assert_eq!(needle.update_clock(sky(0.0), true), 0);
        assert_eq!(needle.update_clock(sky(0.25), true), 16);
        assert_eq!(needle.update_clock(sky(0.5), true), 32);
        assert_eq!(needle.update_clock(sky(-0.25), true), 48);
    }

    #[test]
    fn clock_needles_ease_across_midnight_the_short_way() {
        let mut needle = DialNeedle::default();
        needle.update_clock(
            ClockReading::Sky {
                celestial_angle: 0.95,
            },
            true,
        );
        needle.update_clock(
            ClockReading::Sky {
                celestial_angle: 0.05,
            },
            false,
        );
        assert!(
            needle.velocity > 0.0,
            "forward through midnight, not back through noon"
        );
        assert!((needle.velocity - 0.1 * NEEDLE_PULL * NEEDLE_DAMPING).abs() < 1e-5);
    }

    #[test]
    fn non_finite_readings_keep_the_needle() {
        let mut needle = DialNeedle::default();
        needle.update_clock(
            ClockReading::Sky {
                celestial_angle: 0.25,
            },
            true,
        );
        let frame = needle.update_clock(
            ClockReading::Sky {
                celestial_angle: f32::NAN,
            },
            false,
        );
        assert_eq!(frame, 16);
    }

    /// A catalog with a static compass and `frames` carried compass strip frames.
    fn catalog(frames: u32) -> assets::RuntimeIconCatalog {
        let mut entries = vec![assets::IconEntry {
            identifier: "minecraft:compass".into(),
            metadata: 0,
            sprite: 0,
        }];
        entries.extend((0..frames).map(|frame| assets::IconEntry {
            identifier: "minecraft:compass_atlas".into(),
            metadata: frame,
            sprite: 0,
        }));
        let sprite = assets::IconSprite {
            width: 1,
            height: 1,
            rgba8: vec![0; 4].into(),
        };
        assets::RuntimeIconCatalog::decode(
            &assets::encode_icon_catalog([0; 32], &[sprite], &entries).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn dials_show_their_carried_frame_and_other_items_keep_their_icon() {
        let frames = DialFrames {
            compass: Some(5),
            recovery_compass: None,
            clock: Some(3),
        };
        let carried = catalog(COMPASS_FRAMES);
        assert_eq!(
            frames.icon_key("minecraft:compass", &carried),
            Some(("minecraft:compass_atlas", 5))
        );
        assert_eq!(frames.icon_key("minecraft:stick", &carried), None);
        assert_eq!(
            frames.icon_key("minecraft:clock", &carried),
            None,
            "the catalog carries no clock frames"
        );
        assert_eq!(
            DialFrames::default().icon_key("minecraft:compass", &carried),
            None
        );
        assert_eq!(frames.icon_key("minecraft:compass", &catalog(4)), None);
    }

    #[test]
    fn strips_are_recognised_by_their_native_frame_count() {
        use assets::icon::dial::DialKind::{Clock, Compass};
        assert_eq!(strip_frame_side(Compass, 16, 16 * 32, true), Some(16));
        assert_eq!(strip_frame_side(Clock, 16, 16 * 64, true), Some(16));
        assert_eq!(strip_frame_side(Clock, 16, 16 * 32, true), None);
        // A named strip must hold exactly the native count; an atlas may hold more.
        assert_eq!(strip_frame_side(Compass, 16, 16 * 64, true), None);
        assert_eq!(strip_frame_side(Compass, 16, 16 * 64, false), Some(16));
        assert_eq!(strip_frame_side(Compass, 16, 16 * 31, false), None);
        // Frames are square, so a height that is no whole number of them is no strip.
        assert_eq!(strip_frame_side(Compass, 16, 16 * 32 + 1, false), None);
        assert_eq!(strip_frame_side(Compass, 0, 0, false), None);
    }
}
