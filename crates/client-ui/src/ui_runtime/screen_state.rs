//! Inventory layout preferences and per-window interaction state.

use protocol::CreativeCategory;

mod creative_filter;
mod field_editor;
use creative_filter::CreativeFilterCache;
pub use creative_filter::{CreativeEntries, creative_entries};
pub use field_editor::FieldCaret;
use field_editor::FieldEditor;
pub(in crate::ui_runtime) use field_editor::{FieldKind, FieldLimit};

use super::inventory_drag::InventoryPointer;
use super::presentation::screens::{GRID_CELLS, GRID_COLUMNS};
use ui_screens::container_actions::{InventoryCellHit, SEARCH_TAB};

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
    /// Caret and selection of the focused text field.
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

    use super::field_editor::{MAX_ANVIL_NAME_CHARS, MAX_SEARCH_CHARS};
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
