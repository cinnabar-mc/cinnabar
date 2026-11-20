//! How far a launch reduces graphics after earlier runs ended in the renderer.

/// The reduction one launch runs with. Each level keeps every reduction of the levels below
/// it, so a consumer asks one question of the level and never compares levels itself.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[repr(u8)]
pub enum SafeGraphics {
    /// Full graphics.
    #[default]
    Off = 0,
    /// Discards the saved driver pipeline cache and compiles every pipeline afresh.
    FreshPipelines = 1,
    /// Also compiles pipelines on one worker instead of two.
    SerialCompilation = 2,
    /// Also opens the device as a constrained one and turns optional rendering off.
    Minimal = 3,
}

impl SafeGraphics {
    /// Every level, least reduced first.
    pub const ALL: [Self; 4] = [
        Self::Off,
        Self::FreshPipelines,
        Self::SerialCompilation,
        Self::Minimal,
    ];
    /// The most reduced level; a renderer failure there has nowhere further to step.
    pub const MAX: Self = Self::Minimal;
    /// Environment option forcing a level (`0` to `3`) for one launch; nothing is saved.
    pub const ENV: &'static str = "CINNABAR_SAFE_GRAPHICS";

    /// The saved number of this level.
    pub const fn level(self) -> u8 {
        self as u8
    }

    /// The level saved as `level`; a number past the ladder is its last step.
    pub const fn from_level(level: u8) -> Self {
        match level {
            0 => Self::Off,
            1 => Self::FreshPipelines,
            2 => Self::SerialCompilation,
            _ => Self::Minimal,
        }
    }

    /// The level a forcing option names, or `None` unless it is a number on the ladder.
    pub fn parse(value: &str) -> Option<Self> {
        let level: u8 = value.trim().parse().ok()?;
        (level <= Self::MAX.level()).then(|| Self::from_level(level))
    }

    /// The next step down in graphics; [`Self::MAX`] stays where it is.
    pub const fn raised(self) -> Self {
        Self::from_level(self.level().saturating_add(1))
    }

    /// Whether this launch reduces anything.
    pub const fn is_active(self) -> bool {
        self.level() > Self::Off.level()
    }

    /// Whether the saved driver pipeline cache may be read and written. A cache a driver
    /// crashed on would otherwise be handed back to it at every launch.
    pub const fn uses_driver_pipeline_cache(self) -> bool {
        self.level() < Self::FreshPipelines.level()
    }

    /// Whether pipeline compilation is confined to one worker.
    pub const fn compiles_one_pipeline_at_a_time(self) -> bool {
        self.level() >= Self::SerialCompilation.level()
    }

    /// Whether the device is opened like a constrained one: terrain then takes direct draws,
    /// CPU culling and the compact storage layout, and compiles no culling or unit pipelines.
    pub const fn constrains_device(self) -> bool {
        self.level() >= Self::Minimal.level()
    }

    /// Whether world cameras stay at one coverage sample.
    pub const fn single_sample(self) -> bool {
        self.level() >= Self::Minimal.level()
    }

    /// Whether optional looks are drawn: spatial smoothing, motion blur, Enhanced graphics
    /// and shader packs.
    pub const fn optional_effects(self) -> bool {
        self.level() < Self::Minimal.level()
    }

    /// What this level changes, for the startup log.
    pub const fn summary(self) -> &'static str {
        match self {
            Self::Off => "full graphics",
            Self::FreshPipelines => "driver pipeline cache discarded and unused",
            Self::SerialCompilation => {
                "driver pipeline cache discarded and unused; one pipeline compiles at a time"
            }
            Self::Minimal => {
                "driver pipeline cache discarded and unused; one pipeline compiles at a time; \
                 constrained device (direct terrain draws, CPU culling, compact storage); one \
                 coverage sample; optional effects, Enhanced graphics and shader packs off"
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_round_trip_and_numbers_past_the_ladder_are_its_last_step() {
        for (index, level) in SafeGraphics::ALL.into_iter().enumerate() {
            assert_eq!(usize::from(level.level()), index);
            assert_eq!(SafeGraphics::from_level(level.level()), level);
            assert_eq!(SafeGraphics::parse(&level.level().to_string()), Some(level));
            assert!(!level.summary().is_empty());
        }
        assert_eq!(
            SafeGraphics::ALL[SafeGraphics::ALL.len() - 1],
            SafeGraphics::MAX
        );
        assert_eq!(SafeGraphics::from_level(u8::MAX), SafeGraphics::MAX);
        assert_eq!(SafeGraphics::MAX.raised(), SafeGraphics::MAX);
        assert_eq!(SafeGraphics::Off.raised(), SafeGraphics::FreshPipelines);
        for invalid in ["", "4", "-1", "minimal", "255"] {
            assert_eq!(SafeGraphics::parse(invalid), None, "{invalid}");
        }
        assert_eq!(
            SafeGraphics::parse(" 2\n"),
            Some(SafeGraphics::SerialCompilation)
        );
    }

    #[test]
    fn each_level_keeps_the_reductions_of_the_levels_below_it() {
        let reductions = |level: SafeGraphics| {
            [
                level.is_active(),
                !level.uses_driver_pipeline_cache(),
                level.compiles_one_pipeline_at_a_time(),
                level.constrains_device(),
                level.single_sample(),
                !level.optional_effects(),
            ]
        };
        assert_eq!(reductions(SafeGraphics::Off), [false; 6]);
        assert_eq!(reductions(SafeGraphics::MAX), [true; 6]);
        for pair in SafeGraphics::ALL.windows(2) {
            let (lower, higher) = (reductions(pair[0]), reductions(pair[1]));
            assert!(pair[0] < pair[1]);
            assert_ne!(lower, higher, "{:?} adds nothing", pair[1]);
            for (kept, still) in lower.into_iter().zip(higher) {
                assert!(!kept || still, "{:?} drops a reduction", pair[1]);
            }
        }
    }
}
