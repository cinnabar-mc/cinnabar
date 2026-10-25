//! Connects the Mods menu to its portable service and the runtime's authority.

mod backend;
#[cfg(test)]
mod tests;

use super::registration::removal;
use bevy::prelude::*;
use client_shell::menu::MenuRuntime;
use launcher_host::mod_library::Service;
use std::path::PathBuf;

#[derive(Resource)]
struct ModLibrary(Service);

/// Starts the portable service with the runtime's retirement acknowledgement channel.
pub(super) fn configure(
    app: &mut App,
    mods: PathBuf,
    registration: PathBuf,
    removal: removal::Client,
) {
    match Service::start::<backend::AppBackend>(
        mods,
        registration,
        super::RELOAD_INTERVAL,
        move |receipt| removal.wait(receipt),
    ) {
        Ok(service) => {
            app.insert_resource(ModLibrary(service))
                .add_systems(Update, drive);
        }
        Err(error) => warn!("Mod library unavailable: {error}"),
    }
}

/// Consumes this frame's menu actions and publishes service changes before menu presentation.
fn drive(mut library: ResMut<ModLibrary>, menu: Option<ResMut<MenuRuntime>>) {
    let Some(mut menu) = menu else { return };
    let checks = menu
        .settings_snapshot()
        .0
        .value(launcher::menu::settings_options::SettingId::ModUpdateChecks)
        != 0;
    let automatic = menu
        .settings_snapshot()
        .0
        .value(launcher::menu::settings_options::SettingId::ModAutoUpdate)
        != 0;
    let actions = std::mem::take(&mut menu.mod_actions);
    library.0.drive(actions, &mut menu.mods, checks, automatic);
}
