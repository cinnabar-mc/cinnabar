//! Ordered library commands, update installation and runtime acknowledgements.

use super::{
    Backend, Command, Retirement, Selection,
    files::{import, installed, remove},
    paths::same_path,
    scan::{include_active, scan_cached},
    update,
};
use crate::desktop::picker;
use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use launcher::mods::{Action, Library, UpdatePhase, UpdateProgress};
use std::{collections::BTreeMap, path::Path};

/// Serves commands in order and republishes when the directory or recorded status changes.
pub fn run<B: Backend>(
    mods: &Path,
    registration: &Path,
    incoming: &Receiver<Command>,
    outgoing: &Sender<Library>,
    interval: std::time::Duration,
    mut pick: impl FnMut() -> Result<Option<picker::PickedFile>, String>,
    mut retire: impl FnMut(&B::Receipt) -> Result<B::Permit, String>,
) {
    let mut requests = 0;
    let mut metadata = BTreeMap::new();
    let mut reloads = Vec::<update::PendingReload>::new();
    let mut library = scan_cached::<B>(mods, registration, String::new(), &mut metadata);
    let _ = outgoing.send(library.clone());
    loop {
        let message = match incoming.recv_timeout(interval) {
            Ok(Command::Message(message)) => message,
            Ok(Command::Update(download, finished)) => {
                library.busy = true;
                library.update_progress =
                    vec![UpdateProgress::stage(download.id, UpdatePhase::Installing)];
                let _ = outgoing.send(library.clone());
                match update::install::<B>(mods, &download) {
                    Ok(installed) => {
                        let message = format!(
                            "Installed update {}. Reloading enabled mods…",
                            download.version
                        );
                        requests += 1;
                        let request = format!("mod-update-{}-{requests}", std::process::id());
                        match update::PendingReload::start::<B>(
                            mods,
                            registration,
                            installed,
                            request,
                            finished,
                            std::time::Instant::now(),
                        ) {
                            Ok(Some(pending)) => {
                                reloads.push(pending);
                                message
                            }
                            Ok(None) => format!(
                                "Updated {} to {}.",
                                library
                                    .installed
                                    .iter()
                                    .find(|item| item.id == download.id)
                                    .map(|item| item.file.as_str())
                                    .unwrap_or("mod"),
                                download.version
                            ),
                            Err(error) => error,
                        }
                    }
                    Err(error) => {
                        let _ = finished.send(());
                        error
                    }
                }
            }
            Ok(Command::Action(action)) => {
                if matches!(action, Action::Import | Action::Remove(_)) {
                    library.busy = true;
                    let _ = outgoing.send(library.clone());
                }
                perform::<B>(
                    action,
                    mods,
                    registration,
                    &mut requests,
                    &mut pick,
                    &mut retire,
                )
                .unwrap_or_else(|error| error)
            }
            Err(RecvTimeoutError::Timeout) => library.message.clone(),
            Err(RecvTimeoutError::Disconnected) => return,
        };
        let mut next = scan_cached::<B>(mods, registration, message, &mut metadata);
        let now = std::time::Instant::now();
        reloads.retain_mut(|pending| {
            if let Some(result) = pending.poll(&next, now) {
                next.message = result.unwrap_or_else(|error| error);
                pending.finish();
                false
            } else {
                true
            }
        });
        next.update_progress = reloads
            .iter()
            .map(|pending| UpdateProgress::stage(pending.id, UpdatePhase::Reloading))
            .collect();
        next.busy = !reloads.is_empty();
        if next != library {
            library = next;
            let _ = outgoing.send(library.clone());
        }
    }
}

/// Runs one Mods-tab command; both outcomes are the message the tab shows.
pub fn perform<B: Backend>(
    action: Action,
    mods: &Path,
    registration: &Path,
    requests: &mut u64,
    pick: &mut impl FnMut() -> Result<Option<picker::PickedFile>, String>,
    retire: &mut impl FnMut(&B::Receipt) -> Result<B::Permit, String>,
) -> Result<String, String> {
    let mut entries = installed(mods);
    if !matches!(action, Action::Enable(_) | Action::Remove(_)) {
        include_active(&mut entries, &B::registered(registration));
    }
    let path = |id: u64| {
        entries
            .iter()
            .find(|(item, _)| item.id == id)
            .map(|(_, path)| path.clone())
            .ok_or_else(|| "That mod is no longer installed.".to_owned())
    };
    let mut request = || {
        *requests += 1;
        format!("settings-{}-{}", std::process::id(), *requests)
    };
    match action {
        Action::Import => match pick() {
            Ok(Some(source)) => {
                import::<B>(mods, source.as_ref()).map(|name| format!("Imported {name}."))
            }
            Ok(None) => Ok(String::new()),
            Err(error) => Err(format!(
                "{} The mods folder is {}.",
                error.trim_end(),
                mods.display()
            )),
        },
        Action::Enable(id)
        | Action::Disable(id)
        | Action::Reload(id)
        | Action::MoveUp(id)
        | Action::MoveDown(id)
        | Action::Permission(id, _, _)
        | Action::AllowRequested(id)
        | Action::GrantAll(id)
        | Action::RevokeAll(id) => {
            B::apply(action, registration, Some(&path(id)?), request()).map(|()| String::new())
        }
        Action::ReloadAll => {
            B::apply(action, registration, None, request()).map(|()| String::new())
        }
        Action::Remove(id) => {
            let path = path(id)?;
            let request_id = request();
            let receipt = B::forget(registration, &path, request_id)?;
            let permit = retire(&receipt)?;
            if !installed(mods)
                .iter()
                .any(|(item, current)| item.id == id && same_path(current, &path))
            {
                return Err("That mod changed while removal was pending.".into());
            }
            let companion = B::settings_companion(&path);
            let shared = permit.shared_companion(&companion)
                || B::registered(registration).iter().any(|current| {
                    !same_path(current.component(), &path)
                        && same_path(&B::settings_companion(current.component()), &companion)
                });
            remove::<B>(
                &path,
                (!shared).then_some(companion.as_path()),
                registration,
                &receipt,
                &permit,
            )
            .map(|()| String::new())
        }
        Action::ReadMore(_)
        | Action::Update(_)
        | Action::ViewUpdates
        | Action::OpenUpdate(_)
        | Action::ToggleActive
        | Action::ToggleAvailable
        | Action::Customize(_)
        | Action::ShowAllPermissions(_) => Ok(String::new()),
    }
}
