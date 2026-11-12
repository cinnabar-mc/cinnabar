//! What the world screens present, mirrored into the menu view each frame.

use super::form::{CreateForm, EditForm};
use super::model::Screen;
use super::packs::WorldPacks;
use super::progress::Progress;
use super::prompt::Prompt;
use super::tab::Tab;
use super::world::{UnavailableReason, World};
use bridge::Backend;

/// What the create, edit and modal screens present, mirrored into the menu view each frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorldsView {
    pub screen: Screen,
    pub tab: Tab,
    pub create: CreateForm,
    pub edit: Option<EditForm>,
    /// The saved world behind the edit form (its size and last-saved date).
    pub edited: Option<World>,
    pub form_error: Option<&'static str>,
    pub option_text: std::collections::BTreeMap<super::options::WorldOption, String>,
    /// The edited world's packs once read.
    pub packs: Option<WorldPacks>,
    pub pack_error: Option<String>,
    pub error: Option<String>,
    pub prompt: Option<Prompt>,
    pub progress: Option<Progress>,
    pub busy: bool,
    /// Whether the selected BDS backend can run on this machine.
    pub bds_can_run: bool,
    pub bds_unavailable: Option<UnavailableReason>,
}

impl WorldsView {
    /// A numeric option's text as typed, or its current value before any typing.
    pub fn numeric_text(&self, id: super::options::WorldOption) -> String {
        if let Some(text) = self.option_text.get(&id) {
            return text.clone();
        }
        self.edit
            .as_ref()
            .filter(|_| self.screen == Screen::Edit)
            .map_or(&self.create.options, |edit| &edit.options)
            .get(id)
            .to_string()
    }

    /// Whether the sidebar can open `tab`: pack tabs need a BDS world, the rest need options.
    pub fn tab_available(&self, tab: Tab) -> bool {
        if tab == Tab::General {
            return true;
        }
        if tab.pack_kind().is_some() {
            return if self.edit.is_some() {
                self.edited
                    .as_ref()
                    .is_some_and(|world| world.backend == Backend::Bds)
            } else {
                true
            };
        }
        tab.supported() && self.options_available()
    }

    /// Whether the active form uses the BDS backend that supports world options, with level
    /// data small enough to read and write them.
    pub fn options_available(&self) -> bool {
        if self.edit.is_some() {
            self.edited
                .as_ref()
                .is_some_and(|world| world.backend == Backend::Bds && !world.options_unavailable)
        } else {
            true
        }
    }
}
