//! Connects the Mods menu to its portable service and the runtime's authority.

mod backend;
#[cfg(test)]
mod tests;

use super::registration::removal;
use bevy::prelude::*;
use client_shell::menu::MenuRuntime;
use launcher_host::account_control::AccountControl;
use launcher_host::mod_library::Service;
use std::{path::PathBuf, sync::Arc};

#[derive(Resource)]
struct ModLibrary {
    service: Service,
    marketplace: Option<launcher_host::marketplace::Service>,
}

/// Starts the portable service with the runtime's retirement acknowledgement channel.
pub(super) fn configure(
    app: &mut App,
    mods: PathBuf,
    registration: PathBuf,
    removal: removal::Client,
    authority: Arc<launcher_host::marketplace::Guard>,
) {
    let marketplace = launcher_host::marketplace::Service::start::<backend::AppBackend>(
        mods.clone(),
        authority.clone(),
    )
    .map_err(|error| warn!("Marketplace downloads unavailable: {error}"))
    .ok();
    match Service::start::<backend::AppBackend>(
        mods,
        registration,
        super::RELOAD_INTERVAL,
        move |receipt| removal.wait(receipt),
    ) {
        Ok(service) => {
            service.marketplace_guard(authority);
            app.insert_resource(ModLibrary {
                service,
                marketplace,
            })
            .add_systems(Update, drive);
        }
        Err(error) => warn!("Mod library unavailable: {error}"),
    }
}

/// Consumes this frame's menu actions and publishes service changes before menu presentation.
fn drive(
    mut library: ResMut<ModLibrary>,
    menu: Option<ResMut<MenuRuntime>>,
    mut account: Option<ResMut<client_shell::menu::launcher_account::LauncherAccount>>,
) {
    let context = account.as_deref_mut().and_then(|account| {
        if account.0.account_status() != Some(launcher::menu::auth::AuthState::Authenticated) {
            return None;
        }
        account
            .0
            .account_generation()
            .map(|generation| (account.0.socket_dir().to_owned(), generation))
    });
    let ModLibrary {
        service,
        marketplace,
    } = &mut *library;
    if let Some(marketplace) = marketplace.as_mut() {
        marketplace.configure(context);
    }
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
    if let Some(marketplace) = marketplace {
        let manual = menu.mod_actions.iter().filter_map(|action| {
            let launcher::mods::Action::Update(id) = action else {
                return None;
            };
            menu.mods
                .library
                .installed
                .iter()
                .any(|item| item.id == *id && item.marketplace())
                .then_some(*id)
        });
        marketplace.updates(checks, automatic, manual);
        if let Some(message) = marketplace.message() {
            service.message(message);
        }
        for download in marketplace.downloads() {
            service.marketplace(download);
        }
        service.marketplace_offers(marketplace.offers());
    }
    let actions = std::mem::take(&mut menu.mod_actions);
    service.drive(actions, &mut menu.mods, checks, automatic);
}
