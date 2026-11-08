//! Routes Storage menu actions to the host and local-world controller.

use super::MenuRuntime;
use launcher::menu::{MenuDialog, settings_storage::*};
use launcher_host::settings_storage::{StorageReader, remove_entry};
use std::sync::Arc;
mod worlds;

impl MenuRuntime {
    /// Queues storage measurements; navigation never reads archives on the input thread.
    pub(super) fn refresh_storage(&mut self) {
        if self.storage_reader.is_none() {
            match StorageReader::new() {
                Ok(reader) => {
                    self.storage_reader = Some(reader);
                    Arc::make_mut(&mut self.storage).expanded =
                        CATEGORIES.map(|category| category == "cache");
                }
                Err(error) => {
                    Arc::make_mut(&mut self.storage).error = Some(error.to_string());
                    self.dialog = Some(MenuDialog::StorageError);
                    return;
                }
            }
        }
        let view = Arc::make_mut(&mut self.storage);
        view.loading = true;
        view.error = None;
        if let Err(error) = self
            .storage_reader
            .as_mut()
            .unwrap()
            .request(self.layout.clone())
        {
            view.loading = false;
            view.error = Some(error.to_string());
            self.storage_reader = None;
            self.dialog = Some(MenuDialog::StorageError);
        }
    }

    /// Publishes a completed inventory without waiting for metadata or thumbnail reads.
    pub fn poll_storage(&mut self) {
        if !self.storage.loading {
            return;
        }
        let Some(reader) = &self.storage_reader else {
            return;
        };
        match reader.poll() {
            Ok(Some(mut view)) => {
                view.expanded = self.storage.expanded;
                view.error = self.storage.error.clone().or(view.error);
                self.storage = Arc::new(view);
            }
            Ok(None) => return,
            Err(error) => {
                let view = Arc::make_mut(&mut self.storage);
                view.loading = false;
                view.error = Some(error.to_string());
                self.storage_reader = None;
            }
        }
        if self.storage.error.is_some()
            && self.screen == launcher::menu::MenuScreen::Settings
            && self.settings.section == SECTION_INDEX
        {
            self.dialog = Some(MenuDialog::StorageError);
        }
    }

    /// Handles cache selection and confirms destructive cache operations.
    pub(super) fn activate_storage(&mut self, action: StorageAction) {
        if self.storage.loading
            && matches!(
                action,
                StorageAction::DeletePack(_)
                    | StorageAction::RequestClear
                    | StorageAction::RequestDelete
                    | StorageAction::RequestScreenshots
                    | StorageAction::SelectWorld(_)
            )
        {
            return;
        }
        match action {
            StorageAction::DeletePack(index) => {
                if index >= self.storage.cached.len() {
                    return;
                }
                let storage = Arc::make_mut(&mut self.storage);
                storage.selected = Some(index);
                storage.selected_world = None;
                self.activate_storage(StorageAction::RequestDelete);
            }
            StorageAction::Search => {}
            StorageAction::ClearSearch => {
                self.storage_query.set_text("");
                self.focus_field(launcher::menu::MenuField::StorageQuery);
            }
            StorageAction::Toggle(index) => {
                if let Some(expanded) = Arc::make_mut(&mut self.storage)
                    .expanded
                    .get_mut(usize::from(index))
                {
                    *expanded = !*expanded;
                }
            }
            StorageAction::SelectWorld(index) => {
                let view = Arc::make_mut(&mut self.storage);
                view.selected_world = (view.selected_world != Some(index)
                    && index < view.worlds.len())
                .then_some(index);
                view.selected = None;
            }
            StorageAction::Select(index) => {
                let view = Arc::make_mut(&mut self.storage);
                view.selected_world = None;
                view.selected =
                    (view.selected != Some(index) && index < view.cached.len()).then_some(index);
            }
            StorageAction::RequestClear
            | StorageAction::RequestDelete
            | StorageAction::RequestScreenshots => {
                if self.over_world() || self.is_connecting() || self.session.owns_directory {
                    Arc::make_mut(&mut self.storage).error =
                        Some("Leave the world before clearing downloaded packs.".into());
                    self.dialog = Some(MenuDialog::StorageError);
                    return;
                }
                if action == StorageAction::RequestDelete && self.storage.selected_world.is_some() {
                    self.request_storage_world_delete();
                    return;
                }
                let (root, paths) = match action {
                    StorageAction::RequestDelete => {
                        let Some(item) = self
                            .storage
                            .selected
                            .and_then(|index| self.storage.cached.get(index))
                        else {
                            return;
                        };
                        (
                            self.layout.resource_pack_cache_dir(),
                            vec![item.path.clone()],
                        )
                    }
                    StorageAction::RequestScreenshots => (
                        self.layout.screenshots_dir(),
                        self.storage
                            .screenshots
                            .iter()
                            .map(|item| item.path.clone())
                            .collect(),
                    ),
                    _ => (
                        self.layout.resource_pack_cache_dir(),
                        self.storage
                            .cached
                            .iter()
                            .map(|item| item.path.clone())
                            .collect(),
                    ),
                };
                if paths.is_empty() {
                    return;
                }
                let summary = if action == StorageAction::RequestDelete {
                    self.storage
                        .selected
                        .and_then(|index| self.storage.cached.get(index))
                        .map(|item| format!("{} {}\n{}", item.name, item.version, item.pack_id))
                        .unwrap_or_default()
                } else {
                    format!("All {} downloaded server packs", paths.len())
                };
                Arc::make_mut(&mut self.storage).delete_summary = summary;
                Arc::make_mut(&mut self.storage).pending_delete = Some((root, paths));
                Arc::make_mut(&mut self.storage).deleting_screenshots =
                    action == StorageAction::RequestScreenshots;
                if action != StorageAction::RequestDelete {
                    Arc::make_mut(&mut self.storage).selected = None;
                }
                self.dialog = Some(MenuDialog::StorageDelete);
            }
            StorageAction::OpenFolder(folder) => {
                launcher_host::desktop::open_folder(folder.dir(&self.layout));
            }
            StorageAction::ConfirmDelete => {
                if self.dialog != Some(MenuDialog::StorageDelete) {
                    return;
                }
                self.dialog = None;
                if self.over_world() || self.is_connecting() || self.session.owns_directory {
                    return;
                }
                let Some((root, paths)) = Arc::make_mut(&mut self.storage).pending_delete.take()
                else {
                    return;
                };
                let result = paths.iter().try_for_each(|path| remove_entry(&root, path));
                let expanded = self.storage.expanded;
                self.refresh_storage();
                Arc::make_mut(&mut self.storage).expanded = expanded;
                if let Err(error) = result {
                    Arc::make_mut(&mut self.storage).error =
                        Some(format!("Could not delete stored content: {error}"));
                    self.dialog = Some(MenuDialog::StorageError);
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "settings_storage_tests.rs"]
mod tests;
