//! Mod-library filesystem work and update publication without an engine dependency.

pub mod files;
#[cfg(all(test, target_os = "linux"))]
mod linux_publication_tests;
pub mod paths;
pub mod port;
#[cfg(test)]
mod publication_tests;
pub mod scan;
#[cfg(test)]
mod tests;
pub mod update;
pub mod worker;

use crate::desktop::picker::{self, Filter};
use crossbeam_channel::{Receiver, Sender, unbounded};
use launcher::mods::{Action, COMPONENT_EXTENSION, Library, Snapshot, UpdatePhase, UpdateProgress};
use port::{Backend, Retirement, Selection};
use std::{path::PathBuf, sync::Arc, time::Duration};

/// Ordered work sent to the library thread.
pub enum Command {
    Action(Action),
    Update(crate::mod_updates::Downloaded, Sender<()>),
    Message(String),
}

/// Background library worker and frame-side update checker state.
pub struct Service {
    commands: Sender<Command>,
    published: Receiver<Library>,
    updates: Option<crate::mod_updates::Checker>,
    checks: bool,
    automatic: bool,
    busy: bool,
    progress: Vec<UpdateProgress>,
    installing: Vec<(u64, Receiver<()>)>,
}

impl Service {
    /// Starts one ordered worker, retaining package limits and retirement authority from its owner.
    pub fn start<B: Backend>(
        mods: PathBuf,
        registration: PathBuf,
        interval: Duration,
        mut retire: impl FnMut(&B::Receipt) -> Result<B::Permit, String> + Send + 'static,
    ) -> std::io::Result<Self> {
        let staging = mods.join(".updates");
        let (commands, incoming) = unbounded();
        let (outgoing, published) = unbounded();
        std::thread::Builder::new()
            .name("mod-library".into())
            .spawn(move || {
                worker::run::<B>(
                    &mods,
                    &registration,
                    &incoming,
                    &outgoing,
                    interval,
                    || {
                        picker::pick(&Filter {
                            title: "Import mod",
                            name: "Mods",
                            extensions: &[COMPONENT_EXTENSION],
                            unavailable_hint: "",
                            max_bytes: Some(B::MAX_SOURCE_BYTES as u64),
                        })
                    },
                    &mut retire,
                );
            })?;
        Ok(Self {
            commands,
            published,
            updates: crate::mod_updates::Checker::start(staging, B::MAX_SOURCE_BYTES)
                .map_err(|error| tracing::warn!("Mod update checks unavailable: {error}"))
                .ok(),
            checks: false,
            automatic: false,
            busy: false,
            progress: Vec::new(),
            installing: Vec::new(),
        })
    }

    /// Forwards menu actions and publishes worker and checker results in the same polling turn.
    pub fn drive(
        &mut self,
        actions: Vec<Action>,
        snapshot: &mut Arc<Snapshot>,
        checks: bool,
        automatic: bool,
    ) {
        let mut requested = None;
        for action in actions {
            if let Action::Update(id) = action {
                requested = Some(id);
            } else {
                let _ = self.commands.send(Command::Action(action));
            }
        }
        let latest = self.published.try_iter().last();
        let changed = latest.is_some();
        if let Some(latest) = latest {
            self.busy = latest.busy;
            self.progress = latest.update_progress.clone();
            Arc::make_mut(snapshot).publish(latest);
        }
        let configure = changed || checks != self.checks || automatic != self.automatic;
        self.checks = checks;
        self.automatic = automatic;
        if let Some(checker) = &mut self.updates {
            if configure {
                checker.configure(&snapshot.library.installed, checks, automatic);
            }
            let updates = checker.updates();
            let updates = (snapshot.library.updates != updates).then(|| updates.to_vec());
            let started = requested.is_some_and(|id| checker.install(id));
            let pending = checker.pending();
            let message = checker.take_message().or_else(|| started.then(String::new));
            let downloads = checker.take_downloads();
            let progress = checker.progress();
            for download in downloads {
                let id = download.id;
                let (finished, completion) = unbounded();
                if self
                    .commands
                    .send(Command::Update(download, finished))
                    .is_ok()
                {
                    self.installing.push((id, completion));
                }
            }
            self.installing.retain(|(_, completion)| {
                matches!(
                    completion.try_recv(),
                    Err(crossbeam_channel::TryRecvError::Empty)
                )
            });
            let mut shown = self.progress.clone();
            shown.extend(progress);
            for (id, _) in &self.installing {
                if snapshot.library.installed.iter().any(|item| item.id == *id)
                    && !shown.iter().any(|progress| progress.id == *id)
                {
                    shown.push(UpdateProgress::stage(*id, UpdatePhase::Installing));
                }
            }
            let busy = self.busy || pending.is_some() || !self.installing.is_empty();
            if updates.is_some()
                || snapshot.library.update_progress != shown
                || snapshot.library.busy != busy
                || message.is_some()
            {
                let snapshot = Arc::make_mut(snapshot);
                if let Some(updates) = updates {
                    snapshot.library.updates = updates;
                }
                snapshot.library.update_progress = shown;
                snapshot.library.busy = busy;
                if let Some(message) = message {
                    snapshot.library.message = message.clone();
                    let _ = self.commands.send(Command::Message(message));
                }
            }
        }
    }
}
