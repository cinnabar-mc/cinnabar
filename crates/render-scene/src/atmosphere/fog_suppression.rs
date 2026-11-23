//! Local removal of distance fog that leaves the fog restrictions of vision effects in place.

use super::{AtmosphereFrame, bounded_level};

/// Fog start and end, in blocks, of a frame with its fog removed: beyond any view distance.
const NO_FOG_DISTANCE: f32 = 1.0e6;

impl AtmosphereFrame {
    /// Moves distance fog out of reach so no fragment is fogged, keeping its colour, the camera
    /// medium and every other channel.
    ///
    /// Call it after [`Self::with_vision_effects`] with the same `blindness` and `darkness`.
    /// While either effect is active the frame is returned unchanged, so the restriction the
    /// effects derived from the native fog range keeps closing the view in.
    #[must_use]
    pub fn without_distance_fog(mut self, blindness: f32, darkness: f32) -> Self {
        if bounded_level(blindness) > 0.0 || bounded_level(darkness) > 0.0 {
            return self;
        }
        self.fog_color_start.w = NO_FOG_DISTANCE;
        self.fog_end_time.x = NO_FOG_DISTANCE;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::NO_FOG_DISTANCE;
    use crate::AtmosphereFrame;
    use meshing::CameraMedium;

    const MEDIUMS: [CameraMedium; 3] = [CameraMedium::Air, CameraMedium::Water, CameraMedium::Lava];

    #[test]
    fn removed_fog_reaches_no_fragment_and_keeps_every_other_channel() {
        for medium in MEDIUMS {
            let foggy =
                AtmosphereFrame::from_bedrock_time(6_000.0, 0.25, 0.5).with_camera_medium(medium);
            let clear = foggy.without_distance_fog(0.0, 0.0);
            assert_eq!(clear.fog_start(), NO_FOG_DISTANCE);
            assert!(clear.fog_end() >= clear.fog_start());
            assert_eq!(clear.camera_medium(), medium);
            assert_eq!(clear.fog_color(), foggy.fog_color());
            assert_eq!(clear.sky_horizon(), foggy.sky_horizon());
            assert_eq!(clear.sun_direction(), foggy.sun_direction());
            assert_eq!(clear.rain_level(), foggy.rain_level());
        }
    }

    /// Every blindness and darkness level keeps exactly the restriction the native fog gets.
    #[test]
    fn active_vision_effects_keep_the_native_fog_restriction() {
        for medium in MEDIUMS {
            let native =
                AtmosphereFrame::from_bedrock_time(6_000.0, 0.0, 0.0).with_camera_medium(medium);
            for (blindness, darkness) in
                [(1.0, 0.0), (0.5, 0.0), (0.0, 1.0), (0.0, 0.4), (0.5, 0.5)]
            {
                let effected = native.with_vision_effects(blindness, darkness, 0.0);
                let kept = effected.without_distance_fog(blindness, darkness);
                assert_eq!(kept, effected, "{medium:?} {blindness} {darkness}");
                assert!(kept.fog_end() < 1.0e3, "{medium:?}: {}", kept.fog_end());
            }
        }
    }

    #[test]
    fn partial_blindness_and_darkness_restrict_removed_fog_like_native() {
        let water = AtmosphereFrame::from_bedrock_time(6_000.0, 0.0, 0.0)
            .with_camera_medium(CameraMedium::Water);
        let half_blind = water
            .with_vision_effects(0.5, 0.0, 0.0)
            .without_distance_fog(0.5, 0.0);
        assert_eq!(half_blind.fog_end(), 32.5);
        let dark = water
            .with_vision_effects(0.0, 1.0, 0.0)
            .without_distance_fog(0.0, 1.0);
        assert_eq!(
            dark.fog_end(),
            water.with_vision_effects(0.0, 1.0, 0.0).fog_end()
        );
        assert!(dark.fog_end() < water.fog_end());
    }

    /// Non-finite and negative levels are inactive, exactly as `with_vision_effects` clamps them.
    #[test]
    fn invalid_effect_levels_count_as_inactive() {
        let frame = AtmosphereFrame::from_bedrock_time(6_000.0, 0.0, 0.0);
        for level in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0] {
            assert_eq!(
                frame.without_distance_fog(level, level).fog_start(),
                NO_FOG_DISTANCE,
                "{level}"
            );
        }
    }
}
