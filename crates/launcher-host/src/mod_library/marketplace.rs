//! Verified marketplace replacement, registration migration and acknowledged source retirement.

use super::{Backend, Retirement, Selection, files, paths::same_path};
use crate::marketplace::{Downloaded, Guard};
use std::path::Path;

/// Validates native declarations before publication; only non-expanding updates inherit authority.
pub(crate) fn install<B: Backend>(
    mut download: Downloaded,
    registration: &Path,
    request: String,
    now: u64,
    retire_runtime: &mut impl FnMut(&B::Receipt) -> Result<B::Permit, String>,
) -> String {
    let result = (|| {
        let bytes = download.bytes().map_err(|error| error.to_string())?;
        B::validate_source(&bytes)
            .map_err(|error| format!("Unsupported marketplace component: {error}"))?;
        B::validate_marketplace(&bytes, download.manifest())?;
        let guard = download.guard();
        let previous = download.previous().map(Path::to_path_buf);
        let manifest = download.manifest().clone();
        let subset = if let Some(previous) = &previous {
            let old = guard
                .check(previous, &B::read_source(previous)?, now)
                .map_err(|e| e.to_string())?
                .ok_or("Missing signed predecessor.")?;
            B::marketplace_subset(&old, &manifest)?
        } else {
            false
        };
        if subset {
            download.migrate();
        }
        let target = download
            .install(now, |target| {
                if B::registered(registration)
                    .iter()
                    .any(|entry| same_path(entry.component(), target))
                {
                    B::forget(registration, target, request.clone()).map_err(anyhow::Error::msg)?;
                }
                Ok(())
            })
            .map_err(|e| e.to_string())?;
        if let Some(previous) = previous {
            if subset {
                let previous_entry = B::registered(registration)
                    .into_iter()
                    .find(|entry| same_path(entry.component(), &previous));
                B::replace_marketplace(
                    registration,
                    previous_entry.as_ref(),
                    &target,
                    &manifest,
                    request.clone(),
                )?;
                retire::<B>(
                    &previous,
                    Some(&target),
                    registration,
                    request,
                    &guard,
                    retire_runtime,
                )?;
                crate::marketplace::finish_migration(&target).map_err(|e| e.to_string())?;
                return Ok(format!("Updated marketplace mod to {}.", manifest.version));
            }
            return Ok(format!(
                "Update {} needs permission review. Enable it in My mods when ready; the previous version stays active.",
                manifest.version
            ));
        }
        Ok::<_, String>(launcher::mods::MARKETPLACE_ADDED.to_owned())
    })();
    result.unwrap_or_else(|error| format!("Marketplace installation held: {error}"))
}

/// Waits for final guest writes, preserves settings for a replacement and deletes signed evidence last.
pub(super) fn retire<B: Backend>(
    previous: &Path,
    successor: Option<&Path>,
    registration: &Path,
    request: String,
    guard: &Guard,
    retire_runtime: &mut impl FnMut(&B::Receipt) -> Result<B::Permit, String>,
) -> Result<(), String> {
    let receipt = B::forget(registration, previous, request)?;
    let permit = retire_runtime(&receipt)?;
    B::confirm(&receipt, registration)?;
    permit.confirm()?;
    let companion = B::settings_companion(previous);
    if let Some(successor) = successor {
        let next = B::settings_companion(successor);
        if companion != next && companion.exists() && !next.exists() {
            let parent = next.parent().ok_or("Missing settings directory.")?;
            let mut staged = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
            let mut source = std::fs::File::open(&companion).map_err(|e| e.to_string())?;
            std::io::copy(&mut source, &mut staged).map_err(|e| e.to_string())?;
            staged.as_file().sync_all().map_err(|e| e.to_string())?;
            staged.persist_noclobber(&next).map_err(|e| e.to_string())?;
        }
    }
    let shared = permit.shared_companion(&companion)
        || B::registered(registration).iter().any(|entry| {
            !same_path(entry.component(), previous)
                && same_path(&B::settings_companion(entry.component()), &companion)
        });
    // Keep the source until companion cleanup succeeds, so recovery can retry every file.
    if !shared && companion.try_exists().map_err(|e| e.to_string())? {
        files::remove::<B>(&companion, None, registration, &receipt, &permit)?;
    }
    files::remove::<B>(previous, None, registration, &receipt, &permit)?;
    crate::marketplace::retire_receipt(previous, guard).map_err(|e| e.to_string())
}

/// Validates a managed successor before retiring older versions and selecting it for consent.
pub(super) fn enable<B: Backend>(
    target: &Path,
    registration: &Path,
    request: String,
    guard: &Guard,
    now: u64,
    retire_runtime: &mut impl FnMut(&B::Receipt) -> Result<B::Permit, String>,
) -> Result<String, String> {
    if crate::marketplace::managed(target) {
        let bytes = B::read_source(target)?;
        let manifest = guard
            .check(target, &bytes, now)
            .map_err(|e| e.to_string())?
            .ok_or("Missing signed release.")?;
        B::validate_source(&bytes)?;
        B::validate_marketplace(&bytes, &manifest)?;
        for previous in guard
            .predecessors(target, now)
            .into_iter()
            .filter(|path| path.exists())
        {
            retire::<B>(
                &previous,
                Some(target),
                registration,
                request.clone(),
                guard,
                retire_runtime,
            )?;
        }
    }
    B::apply(
        launcher::mods::Action::Enable(0),
        registration,
        Some(target),
        request,
    )?;
    Ok(String::new())
}
