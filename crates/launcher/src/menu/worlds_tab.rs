//! Local-world control intents and their model inputs.
use super::{LocalWorldCard, MenuField};
use crate::local_worlds::packs::{PackKind, WorldPack};
use crate::local_worlds::world::{Difficulty, World};
use crate::local_worlds::{
    Input, PromptButton, Tab, backend_label, game_mode_label, world_type_label,
};
use bridge::GameMode;

/// A press on a local-world screen or modal; the menu forwards it to the module.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalWorldAction {
    Edit(usize),
    BeginCreate,
    OpenTemplates,
    Import,
    Back,
    Tab(Tab),
    /// Focuses the world name field (create or edit).
    NameField,
    SeedField,
    OptionField(crate::local_worlds::options::WorldOption),
    Option(crate::local_worlds::options::WorldOption, i32),
    GameMode(GameMode),
    Difficulty(Difficulty),
    Flat(bool),
    /// Picks a pack file for the edited world's list of this kind.
    AddPack(PackKind),
    /// Removes the pack at this index of the edited world's list.
    RemovePack(PackKind, usize),
    /// Moves the pack at this index one place toward the front (`true`) or back.
    MovePack(PackKind, usize, bool),
    /// Copies the pack at this index, which the world lists without holding, from the Bedrock game.
    FindPack(PackKind, usize),
    Create,
    PlayFromEdit,
    Delete,
    ConfirmDelete,
    AcceptEula,
    ViewEula,
    Prompt(PromptButton),
}

/// The controls of one pack row, in focus order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PackRowControls {
    /// Present only for a pack the world lists without holding.
    pub find: Option<LocalWorldAction>,
    /// Absent for the first pack.
    pub earlier: Option<LocalWorldAction>,
    /// Absent for the last pack.
    pub later: Option<LocalWorldAction>,
    pub remove: LocalWorldAction,
}

impl PackRowControls {
    /// The controls of pack `index` of `packs`.
    pub fn of(kind: PackKind, index: usize, packs: &[WorldPack]) -> Self {
        Self {
            find: packs
                .get(index)
                .filter(|pack| pack.external)
                .map(|_| LocalWorldAction::FindPack(kind, index)),
            earlier: (index > 0).then_some(LocalWorldAction::MovePack(kind, index, true)),
            later: (index + 1 < packs.len())
                .then_some(LocalWorldAction::MovePack(kind, index, false)),
            remove: LocalWorldAction::RemovePack(kind, index),
        }
    }

    /// The available controls in focus order.
    pub fn actions(self) -> impl Iterator<Item = LocalWorldAction> {
        [self.find, self.earlier, self.later, Some(self.remove)]
            .into_iter()
            .flatten()
    }
}

impl LocalWorldAction {
    /// Every control of a pack tab listing `packs`, in focus order: each row's, then Add.
    pub fn pack_list(kind: PackKind, packs: &[WorldPack]) -> Vec<Self> {
        let mut actions: Vec<_> = (0..packs.len())
            .flat_map(|index| PackRowControls::of(kind, index, packs).actions())
            .collect();
        actions.push(Self::AddPack(kind));
        actions
    }

    /// The text field this press focuses, if any.
    pub fn field(self) -> Option<MenuField> {
        match self {
            Self::NameField => Some(MenuField::WorldName),
            Self::SeedField => Some(MenuField::WorldSeed),
            Self::OptionField(id) => Some(MenuField::WorldOption(id)),
            _ => None,
        }
    }

    /// Converts a launcher control into a local-world model intent.
    pub fn input(self) -> Option<Input> {
        Some(match self {
            Self::Edit(index) => Input::BeginEdit(index),
            Self::BeginCreate => Input::BeginCreate,
            Self::OpenTemplates => Input::OpenTemplates,
            Self::Import => Input::Import,
            Self::Back => Input::Back,
            Self::Tab(tab) => Input::SelectTab(tab),
            Self::NameField | Self::SeedField | Self::OptionField(_) => return None,
            Self::Option(id, value) => Input::SetOption(id, value),
            Self::GameMode(mode) => Input::SetGameMode(mode),
            Self::Difficulty(difficulty) => Input::SetDifficulty(difficulty),
            Self::Flat(flat) => Input::SetFlat(flat),
            Self::AddPack(kind) => Input::AddPack(kind),
            Self::RemovePack(kind, index) => Input::RemovePack(kind, index),
            Self::MovePack(kind, index, earlier) => Input::MovePack(kind, index, earlier),
            Self::FindPack(kind, index) => Input::FindPack(kind, index),
            Self::Create => Input::SubmitCreate,
            Self::PlayFromEdit => Input::PlayFromEdit,
            Self::Delete => Input::RequestDelete,
            Self::ConfirmDelete => Input::ConfirmDelete,
            Self::AcceptEula => Input::AcceptEula,
            Self::ViewEula => Input::OpenEulaLink,
            Self::Prompt(button) => button.input(),
        })
    }
}

/// Presents metadata from the core catalog consistently in Play and Storage.
pub fn world_card(world: &World) -> LocalWorldCard {
    LocalWorldCard {
        name: world.name.clone(),
        game_mode: game_mode_label(world.game_mode).to_owned(),
        world_type: world_type_label(world.generator).to_owned(),
        server: backend_label(world.backend).to_owned(),
        date: civil_date(world.last_played_unix.max(world.created_unix)),
        size: file_size(world.size_bytes),
        thumbnail: None,
    }
}

/// A world's size as the Worlds tab captions it: one decimal in KB, MB or GB.
pub fn file_size(bytes: u64) -> String {
    const UNITS: [&str; 3] = ["KB", "MB", "GB"];
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

/// `month/day/year` of a UTC unix time; empty before the epoch.
pub fn civil_date(unix: i64) -> String {
    if unix <= 0 {
        return String::new();
    }
    // Days-to-civil over 400-year eras (proleptic Gregorian).
    let days = unix / 86_400 + 719_468;
    let era = days / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{month}/{day}/{year}")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dates_render_as_month_day_year() {
        assert_eq!(civil_date(0), "");
        assert_eq!(civil_date(951_782_400), "2/29/2000");
        assert_eq!(civil_date(1_790_553_600), "9/28/2026");
    }

    #[test]
    fn sizes_render_in_the_largest_whole_unit() {
        assert_eq!(file_size(0), "0.0 KB");
        assert_eq!(file_size(512 * 1024), "512.0 KB");
        assert_eq!(file_size(5 * 1024 * 1024 + 300 * 1024), "5.3 MB");
        assert_eq!(file_size(3 * 1024 * 1024 * 1024), "3.0 GB");
    }

    #[test]
    fn pack_rows_offer_only_moves_that_have_somewhere_to_go() {
        use LocalWorldAction::{AddPack, FindPack, MovePack, RemovePack};
        let kind = PackKind::Behaviour;
        let packs = |external: &[bool]| -> Vec<WorldPack> {
            external
                .iter()
                .map(|&external| WorldPack {
                    id: String::new(),
                    version: [1, 0, 0],
                    name: String::new(),
                    description: String::new(),
                    external,
                })
                .collect()
        };
        assert_eq!(
            LocalWorldAction::pack_list(kind, &packs(&[false, true, false])),
            [
                MovePack(kind, 0, false),
                RemovePack(kind, 0),
                FindPack(kind, 1),
                MovePack(kind, 1, true),
                MovePack(kind, 1, false),
                RemovePack(kind, 1),
                MovePack(kind, 2, true),
                RemovePack(kind, 2),
                AddPack(kind),
            ],
            "only a pack the world does not hold can be found"
        );
        assert_eq!(
            LocalWorldAction::pack_list(kind, &packs(&[true])),
            [FindPack(kind, 0), RemovePack(kind, 0), AddPack(kind)]
        );
        assert_eq!(LocalWorldAction::pack_list(kind, &[]), [AddPack(kind)]);
    }
}
