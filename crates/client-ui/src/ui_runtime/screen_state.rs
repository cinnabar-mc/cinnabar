//! Inventory layout preferences and per-window interaction state.

use protocol::CreativeCategory;

mod creative_filter;
use creative_filter::CreativeFilterCache;
pub use creative_filter::{CreativeEntries, creative_entries};

use super::inventory_drag::InventoryPointer;
use super::presentation::screens::{GRID_CELLS, GRID_COLUMNS};
use ui_screens::container_actions::{InventoryCellHit, SEARCH_TAB};

/// Longest creative search text.
const MAX_SEARCH_CHARS: usize = 32;
/// Longest item name an anvil accepts.
const MAX_ANVIL_NAME_CHARS: usize = 50;
/// Bytes the field editor holds: the longest field in four-byte characters.
const FIELD_EDITOR_BYTES: usize = 4 * MAX_ANVIL_NAME_CHARS;
const _: () = assert!(
    MAX_SEARCH_CHARS <= MAX_ANVIL_NAME_CHARS && FIELD_EDITOR_BYTES <= ui::MAX_CHAT_INPUT_BYTES
);

#[derive(Clone, Debug, Default)]
pub struct ScreenState {
    pub pointer: InventoryPointer,
    /// A primary press on the container's close control, awaiting its release.
    pub container_close_armed: bool,
    pub hover: Option<InventoryCellHit>,
    /// Chosen beacon effects; `0` means none.
    pub beacon: (i32, i32),
    /// First visible row of the creative grid.
    pub creative_row: usize,
    pub search: String,
    pub search_focused: bool,
    /// The stonecutter recipe the player picked.
    pub recipe_choice: Option<u32>,
    pub loom_pattern: Option<std::sync::Arc<str>>,
    /// First visible row of the loom pattern grid.
    pub loom_row: usize,
    pub anvil_name: String,
    pub anvil_focused: bool,
    /// The target and resolved name the anvil name box was last filled from.
    pub anvil_target: Option<super::anvil::AnvilNameState>,
    /// The beacon's pyramid level, from its block entity.
    pub beacon_level: Option<u8>,
    pub crafter: CrafterView,
    /// The open mount inventory's entity identifier.
    pub mount_identifier: Option<std::sync::Arc<str>>,
    /// Admitted equipment facts; an unknown mount is not assumed to be a horse.
    pub mount_equipment: assets::entity::mount::MountEquipment,
    /// The open Classic inventory controller's complete preference snapshot.
    pub inventory: Option<inventory::InventoryController>,
    /// Preferences owned by the open furnace controller, independent of later server updates.
    pub furnace: Option<protocol::FurnaceOptions>,
    /// The book, sign-off or lectern reader being shown, if any.
    pub book: Option<super::book_screen::BookState>,
    /// Page of the recipe book, in whole grids.
    pub book_page: usize,
    /// Creative catalog groups the player expanded, by group index.
    pub creative_expanded: std::collections::BTreeSet<u32>,
    /// Scroll offsets of the engine-drawn screen's scroll views, by view key.
    pub container_scroll: std::collections::BTreeMap<String, f64>,
    /// The engine scroll view whose scrollbar thumb is being dragged, and its last pointer axis.
    pub container_scroll_drag: Option<(String, f64)>,
    /// Selected radio tabs of a server pack's storage screen, by group key.
    pub storage_radios: std::collections::BTreeMap<u64, u16>,
    /// Caret and selection of the focused search or anvil field.
    field_editor: FieldEditor,
    window: Option<u64>,
    creative_filter: std::sync::Arc<std::sync::Mutex<Option<CreativeFilterCache>>>,
}

/// What the open crafter's screen shows of its block.
#[derive(Clone, Debug, Default)]
pub struct CrafterView {
    /// Disabled slots as the block entity reports them, one bit per slot.
    pub disabled: u16,
    /// The block's `triggered_bit`.
    pub powered: Option<bool>,
    /// Toggles not yet echoed: the local mask and when it was set, stamped on
    /// the next observation.
    pub pending: Option<(u16, Option<u64>)>,
}

/// The focused search or anvil field's caret and selection. The field's string stays the
/// source of truth: an editor that no longer mirrors it reloads with the caret at the end.
#[derive(Clone, Debug)]
struct FieldEditor {
    editor: ui::ChatEditor,
    /// Changes with every edit and caret move, restarting the caret blink.
    revision: u64,
    /// The field whose text the editor last took up.
    owner: Option<FieldOwner>,
}

/// One search or anvil name field in one container window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FieldOwner {
    anvil: bool,
    window: Option<u64>,
}

impl Default for FieldEditor {
    fn default() -> Self {
        Self {
            editor: ui::ChatEditor::new(FIELD_EDITOR_BYTES)
                .expect("the field editor's size is checked at compile time"),
            revision: 0,
            owner: None,
        }
    }
}

/// Where the focused field's caret and selection sit, as byte offsets into its text.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FieldCaret {
    pub byte: usize,
    pub selection: Option<[usize; 2]>,
    /// Changes with every edit and caret move, restarting the caret blink.
    pub revision: u64,
}

/// How long vanilla's local crafter toggles override the block entity.
pub const CRAFTER_TOGGLE_HOLD_MILLIS: u64 = 1_000;

impl CrafterView {
    /// The disabled slots the screen shows.
    pub fn shown_disabled(&self) -> u16 {
        self.pending.map_or(self.disabled, |(mask, _)| mask)
    }

    pub fn is_disabled(&self, slot: u8) -> bool {
        slot < 9 && self.shown_disabled() & (1 << slot) != 0
    }

    /// Adopts the block entity's state, keeping recent local toggles.
    pub fn observe(&mut self, disabled: u16, powered: Option<bool>, now_millis: u64) {
        self.disabled = disabled & 0x1ff;
        self.powered = powered;
        self.pending = match self.pending {
            Some((mask, None)) => Some((mask, Some(now_millis))),
            Some((mask, Some(at))) if now_millis <= at + CRAFTER_TOGGLE_HOLD_MILLIS => {
                Some((mask, Some(at)))
            }
            _ => None,
        };
    }
}

impl ScreenState {
    /// Whether the retained input state belongs to this window generation.
    pub(super) fn observes_window(&self, generation: Option<u64>) -> bool {
        self.window == generation
    }

    /// Clears per-window choices when a different window becomes current.
    pub fn observe_window(&mut self, generation: Option<u64>) {
        if self.window != generation {
            self.window = generation;
            self.pointer.reset();
            self.container_close_armed = false;
            self.beacon = (0, 0);
            self.search_focused = false;
            self.recipe_choice = None;
            self.furnace = None;
            self.inventory = None;
            self.loom_pattern = None;
            self.loom_row = 0;
            self.anvil_name.clear();
            self.anvil_focused = false;
            self.anvil_target = None;
            self.beacon_level = None;
            self.crafter = CrafterView::default();
            self.mount_identifier = None;
            self.mount_equipment = assets::entity::mount::MountEquipment::Unresolved;
            self.container_scroll.clear();
            self.container_scroll_drag = None;
            self.storage_radios.clear();
            self.creative_expanded.clear();
        }
    }

    /// Installs a preference snapshot with every inventory list at its first page.
    pub(super) fn set_inventory_controller(&mut self, controller: inventory::InventoryController) {
        self.inventory = Some(controller);
        self.creative_row = 0;
        self.book_page = 0;
        self.container_scroll.clear();
        self.container_scroll_drag = None;
    }

    /// Selects a UI category and resets the list position and search focus.
    pub fn select_tab(&mut self, tab: u8) {
        if let Some(tab) = ui_screens::container_actions::inventory_tab(tab) {
            self.inventory
                .get_or_insert_with(|| {
                    inventory::InventoryController::new(
                        Default::default(),
                        inventory::InventoryContext::Personal,
                        false,
                    )
                })
                .options
                .left_tab = tab;
        }
        self.search_focused = tab == SEARCH_TAB;
        self.creative_row = 0;
        self.container_scroll.clear();
        self.container_scroll_drag = None;
    }

    /// Whether a text field owns the keyboard.
    pub fn text_focused(&self) -> bool {
        self.search_focused
            || self.anvil_focused
            || self.book.as_ref().is_some_and(|book| book.editable)
    }

    /// Inserts typed text at the focused field's caret, replacing any selection and dropping
    /// control characters and whatever exceeds the field's length.
    pub fn type_text(&mut self, text: &str) {
        if let Some(book) = self.book.as_mut().filter(|book| book.editable) {
            book.type_text(text);
            return;
        }
        self.edit_field(|editor, limit| {
            let text_chars = editor.as_str().chars().count();
            let selected_chars = editor
                .selected_text()
                .map_or(0, |text| text.chars().count());
            let room = limit.saturating_sub(text_chars - selected_chars);
            let insertion: String = text
                .chars()
                .filter(|ch| !ch.is_control())
                .take(room)
                .collect();
            // Within the field's length, so within the editor's bytes too.
            if !insertion.is_empty() {
                let _ = editor.insert(&insertion);
            }
        });
    }

    pub fn backspace_text(&mut self) {
        if let Some(book) = self.book.as_mut().filter(|book| book.editable) {
            book.backspace();
        } else {
            self.edit_field(|editor, _| editor.backspace());
        }
    }

    /// Applies a caret or deletion key to the focused search or anvil field; see
    /// [`ui::ChatEditor::edit_key`]. `false` when no such field is focused or the key is not
    /// an editing key.
    pub fn edit_text_key(
        &mut self,
        key: semantic_input::KeyboardKey,
        selecting: bool,
        by_word: bool,
    ) -> bool {
        let mut handled = false;
        self.edit_field(|editor, _| handled = editor.edit_key(key, selecting, by_word));
        handled
    }

    /// Selects the focused search or anvil field's whole text; `false` when none is focused.
    pub fn select_all_text(&mut self) -> bool {
        self.edit_field(|editor, _| editor.select_all())
    }

    /// The focused search or anvil field's selected text.
    pub fn selected_text(&self) -> Option<&str> {
        self.mirrored_editor()?.selected_text()
    }

    /// Removes and returns the focused search or anvil field's selected text.
    pub fn cut_text(&mut self) -> Option<String> {
        let mut cut = None;
        self.edit_field(|editor, _| cut = editor.cut_selection());
        cut
    }

    /// Pastes the clipboard's text at the focused search or anvil field's caret, as typing it
    /// would; a clipboard longer than the paste bound pastes nothing. `false` when no such
    /// field is focused.
    pub fn paste_text<C: ui::ChatClipboard>(&mut self, clipboard: &mut C) -> bool {
        if self.focused_field().is_none() {
            return false;
        }
        let bound = super::forms::engine_input::MAX_PASTE_BYTES;
        if let Ok(Some(text)) = clipboard.read_text_bounded(bound)
            && text.len() <= bound
        {
            self.type_text(&text);
        }
        true
    }

    /// The focused search or anvil field's caret, once editing has placed one; a field the
    /// editor does not mirror shows its caret at the end.
    pub fn field_caret(&self) -> Option<FieldCaret> {
        let (field, ..) = self.focused_field()?;
        let revision = self.field_editor.revision;
        Some(match self.mirrored_editor() {
            Some(editor) => FieldCaret {
                byte: editor.cursor_byte(),
                selection: editor.selection().map(|range| [range.start, range.end]),
                revision,
            },
            None => FieldCaret {
                byte: field.len(),
                selection: None,
                revision,
            },
        })
    }

    /// The field editor while it holds the focused field's caret: it took up that field in
    /// this window and the field still has the editor's text.
    fn mirrored_editor(&self) -> Option<&ui::ChatEditor> {
        let (field, _, owner) = self.focused_field()?;
        let FieldEditor {
            editor,
            owner: mirrored,
            ..
        } = &self.field_editor;
        (*mirrored == Some(owner) && editor.as_str() == field).then_some(editor)
    }

    /// The focused search or anvil field, its length in characters and its owner; an editable
    /// book takes typing instead.
    fn focused_field(&self) -> Option<(&str, usize, FieldOwner)> {
        let owner = |anvil| FieldOwner {
            anvil,
            window: self.window,
        };
        if self.book.as_ref().is_some_and(|book| book.editable) {
            None
        } else if self.anvil_focused {
            Some((&self.anvil_name, MAX_ANVIL_NAME_CHARS, owner(true)))
        } else if self.search_focused {
            Some((&self.search, MAX_SEARCH_CHARS, owner(false)))
        } else {
            None
        }
    }

    /// Runs `edit` with the focused field's editor and length, then writes the result back to
    /// the field. A changed search returns the result lists to their top. `false` when no
    /// search or anvil field is focused.
    fn edit_field(&mut self, edit: impl FnOnce(&mut ui::ChatEditor, usize)) -> bool {
        if self.book.as_ref().is_some_and(|book| book.editable) {
            return false;
        }
        let (field, limit, search) = if self.anvil_focused {
            (&mut self.anvil_name, MAX_ANVIL_NAME_CHARS, false)
        } else if self.search_focused {
            (&mut self.search, MAX_SEARCH_CHARS, true)
        } else {
            return false;
        };
        let owner = FieldOwner {
            anvil: !search,
            window: self.window,
        };
        let FieldEditor {
            editor,
            revision,
            owner: mirrored,
        } = &mut self.field_editor;
        if *mirrored != Some(owner) || editor.as_str() != field.as_str() {
            editor.set_text(field);
            *mirrored = Some(owner);
        }
        let before = (editor.cursor_byte(), editor.selection());
        edit(editor, limit);
        let changed = editor.as_str() != field.as_str();
        if changed {
            field.clear();
            field.push_str(editor.as_str());
        }
        if changed || before != (editor.cursor_byte(), editor.selection()) {
            *revision = revision.wrapping_add(1);
        }
        if changed && search {
            self.creative_row = 0;
            self.container_scroll.clear();
            self.container_scroll_drag = None;
        }
        true
    }

    /// Scrolls the loom pattern grid by whole rows.
    pub fn scroll_loom(&mut self, rows: isize, total: usize) {
        let max_row = total
            .div_ceil(super::presentation::screens::LOOM_COLUMNS)
            .saturating_sub(
                super::presentation::screens::LOOM_CELLS
                    / super::presentation::screens::LOOM_COLUMNS,
            );
        self.loom_row = self.loom_row.saturating_add_signed(rows).min(max_row);
    }

    /// Scrolls the creative grid by whole rows, keeping the last page reachable.
    pub fn scroll_creative(&mut self, rows: isize, total: usize) {
        let max_row = total
            .div_ceil(GRID_COLUMNS)
            .saturating_sub(GRID_CELLS / GRID_COLUMNS);
        self.creative_row = self.creative_row.saturating_add_signed(rows).min(max_row);
    }
}

fn tab_category(tab: u8) -> Option<CreativeCategory> {
    match tab {
        0 => Some(CreativeCategory::Construction),
        1 => Some(CreativeCategory::Nature),
        2 => Some(CreativeCategory::Equipment),
        3 => Some(CreativeCategory::Items),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use protocol::{CreativeContentEvent, CreativeGroup, CreativeItem, NetworkItemStack};

    use super::*;

    // Local toggles show until the block entity has had a second to echo them.
    #[test]
    fn crafter_toggles_hold_before_the_block_entity_wins() {
        let mut crafter = CrafterView {
            pending: Some((0b10, None)),
            ..CrafterView::default()
        };
        crafter.observe(0, Some(false), 100);
        assert_eq!(crafter.shown_disabled(), 0b10);
        crafter.observe(0, Some(false), 100 + CRAFTER_TOGGLE_HOLD_MILLIS);
        assert_eq!(crafter.shown_disabled(), 0b10);
        crafter.observe(0b1, Some(true), 101 + CRAFTER_TOGGLE_HOLD_MILLIS);
        assert_eq!(crafter.shown_disabled(), 0b1);
        assert_eq!(crafter.powered, Some(true));
    }

    fn catalog() -> CreativeContentEvent {
        let item = |id: u32, group: u32| CreativeItem {
            creative_network_id: id,
            stack: NetworkItemStack::default(),
            group,
        };
        CreativeContentEvent {
            groups: Arc::from([
                CreativeGroup {
                    category: CreativeCategory::Construction,
                    name: Arc::from("a"),
                    icon: None,
                },
                CreativeGroup {
                    category: CreativeCategory::Nature,
                    name: Arc::from("b"),
                    icon: None,
                },
                CreativeGroup {
                    category: CreativeCategory::CommandOnly,
                    name: Arc::from("c"),
                    icon: None,
                },
            ]),
            items: Arc::from([item(1, 0), item(2, 1), item(3, 2), item(4, 0)]),
            skipped: 0,
        }
    }

    #[test]
    fn tabs_filter_by_category_and_hide_command_only() {
        let catalog = catalog();
        let ids = |tab| {
            creative_entries(&catalog, tab, "", |_| None)
                .iter()
                .map(|item| item.creative_network_id)
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(0), vec![1, 4]);
        assert_eq!(ids(1), vec![2]);
        assert_eq!(ids(SEARCH_TAB), vec![1, 2, 4]);
    }

    #[test]
    fn search_matches_names_case_insensitively() {
        let catalog = catalog();
        let found = creative_entries(&catalog, SEARCH_TAB, "STONE", |item| {
            (item.creative_network_id == 2).then(|| "Stone Brick".to_owned())
        });
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn scrolling_stops_at_the_last_page() {
        let mut state = ScreenState::default();
        state.scroll_creative(50, 100);
        assert_eq!(state.creative_row, 100_usize.div_ceil(9) - 5);
        state.scroll_creative(-50, 100);
        assert_eq!(state.creative_row, 0);
    }

    // Typing goes to the caret, replaces a selection and stops at the field's length; a field
    // rewritten outside editing puts the caret back at its end.
    #[test]
    fn focused_fields_edit_at_the_caret_within_their_length() {
        use semantic_input::KeyboardKey;

        let mut state = ScreenState {
            search_focused: true,
            ..ScreenState::default()
        };
        state.type_text("ingot");
        state.edit_text_key(KeyboardKey::Home, false, false);
        state.creative_row = 3;
        state.type_text("iron ");
        assert_eq!(state.search, "iron ingot");
        assert_eq!(state.creative_row, 0, "a changed search returns to the top");
        assert_eq!(state.field_caret().map(|caret| caret.byte), Some(5));
        state.creative_row = 3;
        state.edit_text_key(KeyboardKey::End, false, false);
        assert_eq!(state.creative_row, 3, "a caret move keeps the results");

        state.type_text(&"x".repeat(MAX_SEARCH_CHARS));
        assert_eq!(state.search.chars().count(), MAX_SEARCH_CHARS);
        assert!(state.select_all_text());
        assert_eq!(state.selected_text(), Some(state.search.as_str()));
        state.type_text("ü");
        assert_eq!(state.search, "ü");

        state.search = "gold".into();
        assert_eq!(state.selected_text(), None);
        assert_eq!(state.field_caret().map(|caret| caret.byte), Some(4));
        state.type_text("en");
        assert_eq!(state.search, "golden");
        state.edit_text_key(KeyboardKey::ArrowLeft, true, true);
        assert_eq!(state.cut_text().as_deref(), Some("golden"));
        assert_eq!(state.search, "");

        state.search_focused = false;
        state.anvil_focused = true;
        state.type_text(&"a".repeat(MAX_ANVIL_NAME_CHARS + 1));
        assert_eq!(state.anvil_name.chars().count(), MAX_ANVIL_NAME_CHARS);
        state.anvil_focused = false;
        assert!(!state.edit_text_key(KeyboardKey::Home, false, false));
        assert_eq!(state.field_caret(), None);
    }

    #[test]
    fn another_field_with_the_same_text_starts_with_its_caret_at_the_end() {
        let mut state = ScreenState {
            search_focused: true,
            ..ScreenState::default()
        };
        state.type_text("iron");
        assert!(state.select_all_text());

        state.observe_window(Some(2));
        state.anvil_name = "iron".into();
        state.anvil_focused = true;
        assert_eq!(state.selected_text(), None);
        assert_eq!(state.field_caret().map(|caret| caret.byte), Some(4));
        state.type_text("y");
        assert_eq!(state.anvil_name, "irony");

        assert!(state.select_all_text());
        state.anvil_focused = false;
        state.search_focused = true;
        state.search = "irony".into();
        assert_eq!(state.selected_text(), None);
        state.backspace_text();
        assert_eq!(state.search, "iron");
    }
}
