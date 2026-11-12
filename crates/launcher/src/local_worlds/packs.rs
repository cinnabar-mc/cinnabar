//! Resource and behaviour packs a saved world carries, and the state behind the pack tabs.

use std::path::PathBuf;

use super::model::{Effect, Event, Input};

/// Which of a world's two pack lists.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PackKind {
    Resource,
    Behaviour,
}

impl PackKind {
    pub const ALL: [Self; 2] = [Self::Resource, Self::Behaviour];

    /// The singular name used in buttons and messages.
    pub const fn noun(self) -> &'static str {
        match self {
            Self::Resource => "resource pack",
            Self::Behaviour => "behaviour pack",
        }
    }
}

/// One pack a world lists. `id` is the manifest UUID text, which names the pack in requests.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorldPack {
    pub id: String,
    pub version: [u32; 3],
    pub name: String,
    pub description: String,
    /// The world lists this pack but holds no copy; the server must supply one itself.
    pub external: bool,
}

impl WorldPack {
    /// `major.minor.patch`.
    pub fn version_text(&self) -> String {
        self.version.map(|part| part.to_string()).join(".")
    }
}

/// A world's packs in server order: the first entry has the highest priority.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WorldPacks {
    pub resource: Vec<WorldPack>,
    pub behaviour: Vec<WorldPack>,
}

impl WorldPacks {
    pub fn list(&self, kind: PackKind) -> &[WorldPack] {
        match kind {
            PackKind::Resource => &self.resource,
            PackKind::Behaviour => &self.behaviour,
        }
    }
}

/// A pack archive on disk that a world is created with.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackFile {
    pub kind: PackKind,
    pub path: PathBuf,
}

/// A pack archive chosen for a world that does not exist yet, with what it holds.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StagedPack {
    pub file: PackFile,
    /// Names the archive's packs as one row.
    pub pack: WorldPack,
}

/// The pack tabs' view of the world being edited, or of the packs a new world will be created with.
#[derive(Debug, Default)]
pub(super) struct PackState {
    world: Option<String>,
    packs: Option<WorldPacks>,
    error: Option<String>,
    /// Whether the tabs are filling `staged` for a world yet to be created.
    creating: bool,
    /// Chosen archives, the first of each kind having the highest priority.
    staged: Vec<StagedPack>,
}

impl PackState {
    /// Starts reading the packs of world `id`, dropping those of any earlier edit.
    pub(super) fn begin(&mut self, id: &str) -> Effect {
        self.clear();
        self.world = Some(id.to_owned());
        Effect::LoadPacks(id.to_owned())
    }

    /// Starts an empty list of packs for a world to create.
    pub(super) fn begin_create(&mut self) {
        self.clear();
        self.creating = true;
    }

    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }

    /// The packs the tabs list: the staged ones while creating, else the edited world's once read.
    pub(super) fn shown(&self) -> Option<WorldPacks> {
        if !self.creating {
            return self.packs.clone();
        }
        let list = |kind| {
            self.staged
                .iter()
                .filter(|staged| staged.file.kind == kind)
                .map(|staged| staged.pack.clone())
                .collect()
        };
        Some(WorldPacks {
            resource: list(PackKind::Resource),
            behaviour: list(PackKind::Behaviour),
        })
    }

    pub(super) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// The staged archives in the order the world installs them, so that each kind's first row
    /// ends up first in the world.
    pub(super) fn files(&self) -> Vec<PackFile> {
        self.staged
            .iter()
            .rev()
            .map(|staged| staged.file.clone())
            .collect()
    }

    /// Takes the worker's answer to a pack request; other worlds' answers are ignored.
    pub(super) fn apply(&mut self, event: Event) {
        match event {
            Event::Packs { id, packs } => self.loaded(&id, packs),
            Event::PackFailed { id, message } => self.failed(&id, message),
            Event::PackStaged(staged) => self.staged(staged),
            Event::PackStageFailed(message) => self.stage_failed(message),
            _ => {}
        }
    }

    /// Takes the archive the worker read, replacing a staged pack with the same id; `None` is a
    /// cancelled dialog.
    fn staged(&mut self, staged: Option<StagedPack>) {
        let Some(staged) = staged.filter(|_| self.creating) else {
            return;
        };
        self.staged
            .retain(|held| held.file.kind != staged.file.kind || held.pack.id != staged.pack.id);
        self.staged.insert(0, staged);
    }

    /// The index in `staged` of row `index` of the list of `kind`.
    fn staged_index(&self, kind: PackKind, index: usize) -> Option<usize> {
        self.staged
            .iter()
            .enumerate()
            .filter(|(_, staged)| staged.file.kind == kind)
            .nth(index)
            .map(|(at, _)| at)
    }

    /// Takes row `index` of the list of `kind` off the staged archives.
    fn unstage(&mut self, kind: PackKind, index: usize) {
        if let Some(at) = self.staged_index(kind, index) {
            self.staged.remove(at);
        }
    }

    /// Swaps row `index` of the list of `kind` with the one before it (`earlier`) or after it.
    fn restage(&mut self, kind: PackKind, index: usize, earlier: bool) {
        let other = if earlier {
            index.checked_sub(1)
        } else {
            Some(index + 1)
        };
        if let (Some(a), Some(b)) = (
            self.staged_index(kind, index),
            other.and_then(|other| self.staged_index(kind, other)),
        ) {
            self.staged.swap(a, b);
        }
    }

    /// The loaded world, once its packs are known.
    fn ready(&self) -> Option<(&str, &WorldPacks)> {
        Some((self.world.as_deref()?, self.packs.as_ref()?))
    }

    /// The worker request a pack input asks for, if any; edits of staged packs happen at once.
    pub(super) fn request(&mut self, input: Input) -> Option<Effect> {
        match input {
            Input::AddPack(kind) => self.add(kind),
            Input::RemovePack(kind, index) => self.remove(kind, index),
            Input::MovePack(kind, index, earlier) => self.shift(kind, index, earlier),
            Input::FindPack(kind, index) => self.find(kind, index),
            _ => None,
        }
    }

    /// Asks to copy the pack at `index` of the list of `kind` from the Bedrock game's folders, when
    /// the world lists it without holding a copy.
    fn find(&mut self, kind: PackKind, index: usize) -> Option<Effect> {
        let (id, packs) = self.ready()?;
        let pack = packs.list(kind).get(index).filter(|pack| pack.external)?;
        let effect = Effect::FindPack {
            id: id.to_owned(),
            kind,
            pack: pack.id.clone(),
        };
        self.error = None;
        Some(effect)
    }

    /// Asks to pick and add a pack to the list of `kind`.
    pub(super) fn add(&mut self, kind: PackKind) -> Option<Effect> {
        if self.creating {
            self.error = None;
            return Some(Effect::StagePack(kind));
        }
        let (id, _) = self.ready()?;
        let effect = Effect::AddPack {
            id: id.to_owned(),
            kind,
        };
        self.error = None;
        Some(effect)
    }

    /// Asks to remove the pack at `index` of the list of `kind`.
    pub(super) fn remove(&mut self, kind: PackKind, index: usize) -> Option<Effect> {
        if self.creating {
            self.unstage(kind, index);
            return None;
        }
        let (id, packs) = self.ready()?;
        let pack = packs.list(kind).get(index)?.id.clone();
        let effect = Effect::RemovePack {
            id: id.to_owned(),
            kind,
            pack,
        };
        self.error = None;
        Some(effect)
    }

    /// Asks to move the pack at `index` of the list of `kind` one place toward the front
    /// (`earlier`) or the back.
    pub(super) fn shift(&mut self, kind: PackKind, index: usize, earlier: bool) -> Option<Effect> {
        if self.creating {
            self.restage(kind, index, earlier);
            return None;
        }
        let (id, packs) = self.ready()?;
        let list = packs.list(kind);
        let to = if earlier {
            index.checked_sub(1)?
        } else {
            index + 1
        };
        if to >= list.len() {
            return None;
        }
        let effect = Effect::MovePack {
            id: id.to_owned(),
            kind,
            pack: list.get(index)?.id.clone(),
            to,
        };
        self.error = None;
        Some(effect)
    }

    /// Takes the packs the worker read or changed; other worlds' answers are ignored.
    fn loaded(&mut self, id: &str, packs: WorldPacks) {
        if self.world.as_deref() == Some(id) {
            self.packs = Some(packs);
        }
    }

    /// Records why a pack request failed, keeping the last known lists.
    fn failed(&mut self, id: &str, message: String) {
        if self.world.as_deref() == Some(id) {
            self.error = Some(message);
        }
    }

    /// Records why choosing a pack for the new world failed.
    fn stage_failed(&mut self, message: String) {
        if self.creating {
            self.error = Some(message);
        }
    }
}
