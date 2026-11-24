//! Prepared packet mods belong to new network sessions, never the frame callback host.

use super::registration::{Entry, RegisteredHost, Retired, Watcher};
use bevy::prelude::*;
use client_extensions::modding::packet_relay::{PacketModLoader, PacketModSelection};
use mod_host::packet_mod::CompiledPacketMod;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};

#[cfg(test)]
mod tests;

struct Prepared {
    registration: RegisteredHost,
    program: Arc<CompiledPacketMod>,
    authorized: bool,
}

/// The registration owner's packet programs and the selected source used by joins.
#[derive(Resource, Default)]
pub(crate) struct PacketMods {
    loader: PacketModLoader,
    programs: BTreeMap<PathBuf, Prepared>,
    selected: Option<[u8; 32]>,
    environment: Option<PacketModSelection>,
    environment_selected: bool,
    retired: Vec<Arc<CompiledPacketMod>>,
    pub(super) panel: super::packet_panel::State,
    /// Advances whenever what [`Self::authorize`] reads or decides changes: the programs, their
    /// authorization, the selection or the developer fallback. Writers compare first.
    revision: u64,
    /// The entry list and revision of the last [`Self::authorize_shared`] that ran.
    authorized_for: Option<(Arc<[Entry]>, u64)>,
}

/// Starts compilation for an explicitly selected developer packet mod before the first join.
pub(crate) fn from_environment() -> PacketMods {
    let mut mods = PacketMods::default();
    if let Some(selection) = client_extensions::modding::grants::environment_packet_mod() {
        mods.select_environment(selection);
    } else if let Some(set) = std::env::var_os(client_extensions::modding::multi::SET_ENV)
        && let Ok(entries) = client_extensions::modding::multi::read_set(Path::new(&set))
        && let Some(selection) = client_extensions::modding::multi::packet_mod_entry(&entries)
    {
        mods.select_environment(selection);
    }
    mods
}

impl PacketMods {
    /// Captures the cache mode for enabled registration and developer-selected packet authority.
    pub(crate) fn connection_cache(
        cache: &client_runtime::blob_cache::ClientBlobCacheOwner,
        packets: Option<&Self>,
        active_grants: impl IntoIterator<Item = mod_host::ModGrants>,
    ) -> client_runtime::blob_cache::ClientBlobCacheOwner {
        let grants: Vec<_> = active_grants
            .into_iter()
            .chain(packets.and_then(|packets| packets.loader.selected_grants()))
            .collect();
        cache.for_session(mod_host::ModGrants::enables_upstream_client_cache(&grants))
    }

    /// Reads startup consent before the registration worker and direct connection are running.
    pub(crate) fn startup_cache(
        &self,
        cache: &client_runtime::blob_cache::ClientBlobCacheOwner,
        config: &Path,
    ) -> client_runtime::blob_cache::ClientBlobCacheOwner {
        Self::connection_cache(
            cache,
            Some(self),
            super::registration::selection::registered(config)
                .into_iter()
                .filter(|registered| registered.entry.enabled)
                .map(|registered| registered.entry.grants),
        )
    }

    /// Whether snapshot construction is needed for this live packet session.
    pub(super) fn native_frame_available(&self) -> bool {
        self.loader.native_frame_available()
    }

    /// Exchanges native input and local effects without waiting for the packet worker.
    pub(super) fn native_frame(
        &self,
        frame: Option<mod_host::NativeFrame>,
    ) -> Option<client_extensions::modding::packet_relay::FrameOutput> {
        self.loader.native_frame(frame)
    }

    /// Shares only grant-gated panel state, without entering guest code.
    pub(super) fn panel_handle(&self) -> Option<mod_host::packet_mod::panel::PanelHandle> {
        self.loader.panel()
    }

    /// Last explicit opening request from the live packet surface, while it remains open.
    pub(super) fn panel_open_order(&self) -> Option<u64> {
        let output = self.panel_handle()?.output();
        output.open.then_some(output.open_order)
    }

    /// Records a developer-selected source as the fallback when Settings selects no packet mod.
    pub(super) fn select_environment(&mut self, selection: PacketModSelection) {
        self.revision += 1;
        self.environment = Some(selection.clone());
        if self.selected.is_none() {
            if let Some(old) = self.loader.select(Some(selection)) {
                self.retired.push(old);
            }
            self.environment_selected = true;
        }
    }
    /// Captures revocable authority only when login was configured for complete chunk bodies.
    pub(crate) fn session_relay(
        &self,
        upstream_client_cache: bool,
    ) -> Option<Arc<dyn protocol::PacketRelay>> {
        if upstream_client_cache {
            if self.loader.selected_grants().is_some() {
                bevy::log::warn!("Reconnect to start the packet mod with complete chunk data.");
            }
            return None;
        }
        self.loader.session_relay()
    }

    /// Whether this component remains admitted under its current grants.
    pub(super) fn admitted(&self, path: &Path) -> bool {
        self.programs
            .get(path)
            .is_some_and(|program| program.authorized)
    }

    /// Adds one immutable prepared source; the list's order chooses the source used by joins.
    pub(super) fn publish(
        &mut self,
        entry: Entry,
        identity: [u8; 32],
        program: Arc<CompiledPacketMod>,
    ) -> Option<Arc<CompiledPacketMod>> {
        self.revision += 1;
        self.programs
            .insert(
                entry.component.clone(),
                Prepared {
                    registration: RegisteredHost { entry, identity },
                    program,
                    authorized: true,
                },
            )
            .map(|old| old.program)
    }

    /// Revokes removed grants immediately, independently of compilation and reload progress.
    pub(super) fn authorize(&mut self, entries: &[Entry]) -> Vec<Arc<CompiledPacketMod>> {
        for program in self.programs.values_mut() {
            let authorized = entries.iter().any(|entry| {
                entry.enabled
                    && entry.component == program.registration.entry.component
                    && entry.grants == program.registration.entry.grants
            });
            if program.authorized != authorized {
                program.authorized = authorized;
                self.revision += 1;
            }
        }
        self.select(entries);
        std::mem::take(&mut self.retired)
    }

    /// [`Self::authorize`] for the frame's shared entry list. Authorizing is idempotent, so when
    /// the list (the same allocation) and everything it is checked against are unchanged since the
    /// last run, and nothing waits for retirement, this returns nothing without running it again.
    pub(super) fn authorize_shared(
        &mut self,
        entries: &Arc<[Entry]>,
    ) -> Vec<Arc<CompiledPacketMod>> {
        if self.retired.is_empty()
            && self
                .authorized_for
                .as_ref()
                .is_some_and(|(last, revision)| {
                    Arc::ptr_eq(last, entries) && *revision == self.revision
                })
        {
            return Vec::new();
        }
        let retired = self.authorize(entries);
        self.authorized_for = Some((Arc::clone(entries), self.revision));
        retired
    }

    /// Reorders accepted sources without starting or compiling a guest on the frame thread.
    pub(super) fn select(&mut self, entries: &[Entry]) {
        let selected = entries
            .iter()
            .filter(|entry| entry.enabled)
            .find_map(|entry| {
                self.programs
                    .get(&entry.component)
                    .filter(|program| program.authorized)
            });
        let identity = selected.map(|program| program.registration.identity);
        if identity == self.selected
            && (identity.is_some() || self.environment_selected == self.environment.is_some())
        {
            return;
        }
        self.revision += 1;
        self.selected = identity;
        let retired = if let Some(program) = selected {
            self.environment_selected = false;
            self.loader.select_prepared(
                PacketModSelection {
                    path: program.registration.entry.component.clone(),
                    grants: program.registration.entry.grants.clone(),
                },
                Arc::clone(&program.program),
            )
        } else {
            self.environment_selected = self.environment.is_some();
            self.loader.select(self.environment.clone())
        };
        if let Some(old) = retired {
            self.retired.push(old);
        }
    }

    /// Removes disabled sources and moves their executable resources to off-frame retirement.
    pub(super) fn remove_disabled(&mut self, entries: &[Entry]) -> Vec<Arc<CompiledPacketMod>> {
        let removed: Vec<_> = self
            .programs
            .keys()
            .filter(|path| {
                !entries
                    .iter()
                    .any(|entry| entry.enabled && &entry.component == *path)
            })
            .cloned()
            .collect();
        if !removed.is_empty() {
            self.revision += 1;
        }
        removed
            .into_iter()
            .filter_map(|path| self.programs.remove(&path).map(|program| program.program))
            .collect()
    }

    /// Removes a packet program when its registration changes to a frame component.
    pub(super) fn remove(&mut self, path: &Path) -> Option<Arc<CompiledPacketMod>> {
        let removed = self.programs.remove(path).map(|program| program.program);
        if removed.is_some() {
            self.revision += 1;
        }
        removed
    }

    /// Source identities join the watcher's existing acknowledgment protocol.
    pub(super) fn identities(&self) -> impl Iterator<Item = (PathBuf, [u8; 32])> + '_ {
        self.programs
            .iter()
            .filter(|(_, program)| program.authorized)
            .map(|(path, program)| (path.clone(), program.registration.identity))
    }
}

/// Installs a packet candidate alongside frame mods, with retirement on the existing worker.
pub(super) fn publish(
    world: &mut World,
    entry: Entry,
    identity: [u8; 32],
    program: Arc<CompiledPacketMod>,
) {
    world.init_resource::<PacketMods>();
    if let Some(old) = world
        .resource_mut::<PacketMods>()
        .publish(entry, identity, program)
    {
        world.resource::<Watcher>().retire(Retired::Packet(old));
    }
}
