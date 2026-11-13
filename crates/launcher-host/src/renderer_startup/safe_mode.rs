//! Chooses each launch's [`SafeGraphics`] level from how the previous run ended.
//!
//! A platform with one graphics API cannot recover from a renderer crash by excluding the
//! API. It steps down a ladder of reductions instead: each launch after a run that died in
//! the renderer uses the next level, a session the renderer survives clears the ladder, and a
//! crash straight after clearing returns to the level that worked and stays there until the
//! client version or the player's Graphics API choice changes.

use std::path::Path;

use render_api::SafeGraphics;
use serde::{Deserialize, Serialize};

use super::{
    breadcrumb::{self, Breadcrumb, Previous, RunEnd, RunMarker, RunStage},
    exit_record::{ExitKind, ExitRecord},
    owner::Owner,
};

/// The ladder's file name inside the renderer's data root.
const FILE: &str = "renderer-safe-graphics.json";

/// What the previous run says about the renderer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    /// The run died in the renderer.
    RendererDeath,
    /// The renderer survived a whole session.
    Healthy,
    /// Neither: no earlier run, an unrelated crash, or a session too short to prove anything.
    Inconclusive,
}

/// Judges the previous run from its marker and the platform's record of its exit.
///
/// A cause the run recorded itself decides alone. A run that vanished counts only when the
/// platform reports a native fault; without any platform record it counts only in the two
/// stages that are almost entirely pipeline compilation, where a player rarely closes the app.
pub fn outcome(previous: Option<&RunMarker>, exit: ExitKind) -> Outcome {
    let Some(run) = previous else {
        return Outcome::Inconclusive;
    };
    let died = match run.end {
        Some(RunEnd::GpuFailure | RunEnd::RendererPanic) => true,
        Some(RunEnd::Clean | RunEnd::Panic) => false,
        None => match exit {
            ExitKind::NativeCrash => true,
            ExitKind::Unknown => matches!(run.stage, RunStage::Startup | RunStage::WarmUp),
            ExitKind::Other => false,
        },
    };
    if died {
        Outcome::RendererDeath
    } else if run.healthy {
        Outcome::Healthy
    } else {
        Outcome::Inconclusive
    }
}

/// The ladder between launches.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Ladder {
    /// The level the next launch uses.
    pub level: SafeGraphics,
    /// The level a healthy session was cleared from, until full graphics prove themselves.
    pub cleared_from: Option<SafeGraphics>,
    /// Full graphics crashed again after a clear, so healthy sessions no longer clear.
    pub pinned: bool,
}

impl Ladder {
    /// The ladder after a run with `outcome`.
    #[must_use]
    pub fn after(self, outcome: Outcome) -> Self {
        match outcome {
            Outcome::Inconclusive => self,
            Outcome::RendererDeath => match self.cleared_from {
                Some(worked) if !self.level.is_active() => Self {
                    level: worked,
                    cleared_from: None,
                    pinned: true,
                },
                _ => Self {
                    level: self.level.raised(),
                    ..self
                },
            },
            Outcome::Healthy if self.pinned => self,
            Outcome::Healthy => Self {
                level: SafeGraphics::Off,
                cleared_from: self.level.is_active().then_some(self.level),
                pinned: false,
            },
        }
    }
}

/// The ladder as saved, with the client version and Graphics API choice it belongs to.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    level: u8,
    cleared_from: Option<u8>,
    pinned: bool,
    /// The level the player was last told about, so a level that stays is announced once.
    notified: u8,
    version: String,
    graphics_api_revision: u64,
}

impl Saved {
    fn ladder(&self) -> Ladder {
        Ladder {
            level: SafeGraphics::from_level(self.level),
            cleared_from: self.cleared_from.map(SafeGraphics::from_level),
            pinned: self.pinned,
        }
    }
}

/// What a launch needs to know before it creates its renderer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SafeStart {
    pub level: SafeGraphics,
    /// Healthy sessions no longer clear the level; see [`Ladder::pinned`].
    pub kept: bool,
    /// The player has not been told about this level yet.
    pub notify: bool,
    /// Why the level was chosen, for the startup log.
    pub reason: String,
}

impl SafeStart {
    /// Full graphics on a platform that keeps no ladder.
    pub fn unused() -> Self {
        Self {
            level: SafeGraphics::Off,
            kept: false,
            notify: false,
            reason: "this platform keeps no safe graphics ladder".to_owned(),
        }
    }

    /// A level the launch was told to use. The ladder is neither read nor saved, and the
    /// previous run's marker under `root` is deleted: this launch keeps none, so its exit must
    /// not be read later as that run's.
    pub fn forced(root: &Path, level: SafeGraphics) -> Self {
        breadcrumb::discard(root);
        Self {
            level,
            kept: false,
            notify: level.is_active(),
            reason: format!("forced by {}", SafeGraphics::ENV),
        }
    }
}

/// What one launch tells [`start`] about itself.
#[derive(Clone, Copy, Debug)]
pub struct Launch<'a> {
    /// The client version; a different one starts the ladder afresh.
    pub version: &'a str,
    /// The saved Graphics API choice's revision; choosing an API again clears the ladder.
    pub graphics_api_revision: u64,
    /// The platform's record of the last exit, when it keeps one.
    pub exit: Option<ExitRecord>,
    /// Now, in milliseconds since the Unix epoch.
    pub now_ms: u64,
}

/// Judges the previous run, saves the ladder's next state and starts this run's marker.
///
/// Unreadable state is treated as absent and failed writes are logged: recovery must never
/// stop a launch.
pub fn start(root: &Path, launch: Launch<'_>) -> SafeStart {
    let owner = Owner::current().ok();
    let saved: Saved = super::load(&root.join(FILE))
        .ok()
        .flatten()
        .unwrap_or_default();
    let previous = breadcrumb::previous(root);
    let same_settings = saved.version == launch.version
        && saved.graphics_api_revision == launch.graphics_api_revision;
    let (ladder, reason) = if !same_settings {
        let reason = if saved.ladder() == Ladder::default() {
            "no renderer failure recorded"
        } else {
            "cleared for a new client version or Graphics API choice"
        };
        (Ladder::default(), reason.to_owned())
    } else {
        step(saved.ladder(), &previous, launch.exit, owner.as_ref())
    };
    let notify =
        ladder.level.is_active() && !(same_settings && saved.notified == ladder.level.level());
    let next = Saved {
        level: ladder.level.level(),
        cleared_from: ladder.cleared_from.map(SafeGraphics::level),
        pinned: ladder.pinned,
        notified: ladder.level.level(),
        version: launch.version.to_owned(),
        graphics_api_revision: launch.graphics_api_revision,
    };
    if next != saved
        && let Err(error) = super::save(&root.join(FILE), &next)
    {
        tracing::warn!(%error, "could not save the safe graphics level");
    }
    breadcrumb::activate(Breadcrumb::begin(root, ladder.level, launch.now_ms, owner));
    SafeStart {
        level: ladder.level,
        kept: ladder.pinned,
        notify,
        reason,
    }
}

/// Applies the previous run to `ladder` and says why, for the startup log.
fn step(
    ladder: Ladder,
    previous: &Previous,
    exit: Option<ExitRecord>,
    owner: Option<&Owner>,
) -> (Ladder, String) {
    let run = match previous {
        Previous::Missing => return (ladder, "no earlier run recorded".to_owned()),
        Previous::Unreadable => {
            return (ladder, "the previous run's marker is unreadable".to_owned());
        }
        // An earlier run of this same process ended without the process dying.
        Previous::Marker(run) if owner.is_some() && run.owner.as_ref() == owner => {
            return (ladder, "the previous run was in this process".to_owned());
        }
        Previous::Marker(run) => run,
    };
    let kind = ExitRecord::kind_for(exit, run.started_ms);
    let end = match (run.end, exit) {
        (Some(end), _) => format!("{end:?}"),
        (None, Some(exit)) if kind != ExitKind::Unknown => {
            format!("exit reason {} status {}", exit.reason, exit.status)
        }
        (None, _) => "no exit record".to_owned(),
    };
    let stage = run.stage.name();
    let outcome = outcome(Some(run), kind);
    let next = ladder.after(outcome);
    let reason = match outcome {
        Outcome::RendererDeath if next.pinned && !ladder.pinned => format!(
            "full graphics failed in the renderer again during {stage} ({end}); keeping the level that worked"
        ),
        Outcome::RendererDeath => {
            format!("the previous run ended in the renderer during {stage} ({end})")
        }
        Outcome::Healthy if ladder.pinned => {
            "kept after a healthy session: full graphics failed twice on this version".to_owned()
        }
        Outcome::Healthy => "the previous session ran without a renderer failure".to_owned(),
        Outcome::Inconclusive => {
            format!("unchanged: the previous run ended during {stage} ({end})")
        }
    };
    (next, reason)
}

#[cfg(test)]
mod tests;
