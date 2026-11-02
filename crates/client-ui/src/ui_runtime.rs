//! Client presentation state and commands, with player authority borrowed from its owner.

pub mod anvil;
pub mod bed;
pub mod book_screen;
mod boss;
pub mod chat_completion;
pub mod chat_send;
pub mod crafting_observation;
pub mod credits;
pub mod data_driven;
mod death;
pub mod emotes;
pub mod presentation_snapshot;

mod error;
pub mod event_apply;
pub use error::UiRuntimeError;
pub mod forms;
pub mod gameplay_authority;
pub mod gameplay_hud;
pub mod grindstone;
pub mod hud_adapter;
pub mod interaction;
pub mod inventory_actions;
pub mod inventory_drag;
pub mod inventory_ingress;

mod food;
mod furnace_options;
mod inventory_options;
mod item_activation;
pub mod item_facts;
pub mod json_ui_assets;
pub mod mob_effects;
mod mod_inventory;
pub mod platform_clipboard;
pub mod player_roster;
pub mod presentation;
pub mod raw_text_resolution;
pub mod render_adapter;
pub mod scene_stack;
pub mod scoreboard_adapter;
pub mod screen_recipes;
pub mod screen_state;
pub mod session;
pub mod session_data;
pub mod sign_editor;
pub mod use_on_identity_evidence;

pub use forms::{
    FORM_HANDOFF_GRACE_MILLIS, FormRespondError, FormTransportError, LocalFormAction,
    ServerFormEntry, ServerFormIdentity, ServerFormStore, flush_form_response,
};

pub use gameplay_authority::Translator;
pub use interaction::FastTransferAction;
#[cfg(any(test, feature = "test-support"))]
pub use interaction::dispatch_inventory_click;
pub use interaction::{ChatFlushError, flush_chat_sends, flush_inventory_send};

use std::sync::Arc;

use protocol::{ActorAttribute, BlockCrackEvent, EquipmentEvent, InventoryAuthority, UiEvent};
use semantic_input::InputContext;
use ui::{
    BossBarStore, ChatAutocompleteRequest, ChatClipboard, ChatEditor, ChatEditorError, ChatHistory,
    ChatPasteError, ChatStore, HudStore, ScoreboardStore,
};

use self::gameplay_hud::GameplayHudState;
use inventory::inventory_ledger::PlayerInventoryLedger;
use inventory::inventory_router::{EquipmentRoute, InventoryRouterError};

pub use platform_clipboard::PlatformClipboard;

#[derive(Clone, Debug)]
pub struct SequencedUiEvent {
    pub session_id: u64,
    pub fifo_sequence: u64,
    pub local_millis: u64,
    /// Ordering metadata only. Timed HUD state stamps exclusively from
    /// `local_millis`; a populated tick on a timed family is rejected by
    /// `apply` instead of being converted, so the two clocks can never mix.
    pub server_tick: Option<u64>,
    pub event: UiEvent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SequencedBlockCrackEvent {
    pub session_id: u64,
    pub fifo_sequence: u64,
    pub dimension: i32,
    pub event: BlockCrackEvent,
}

#[derive(Clone, Debug)]
pub struct SequencedLocalAttributes {
    pub session_id: u64,
    pub fifo_sequence: u64,
    pub local_millis: u64,
    pub server_tick: u64,
    pub attributes: Arc<[ActorAttribute]>,
}

use inventory::SequencedLocalEquipment;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UiApplyOutcome {
    Applied,
    IgnoredByReceiveStore,
    // The typed document requires localization, scoreboard, or selector state that is not wired.
    IgnoredUnresolvedRawText,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UiAuthorityTransition {
    consumes_text: bool,
    requested_context: InputContext,
}

impl UiAuthorityTransition {
    pub const fn ui_consumed_text(self) -> bool {
        self.consumes_text
    }

    pub const fn requested_input_context(self) -> InputContext {
        self.requested_context
    }
}

#[derive(Clone, Debug)]
pub struct UiRuntime {
    /// Server state is replaced as one value when the server session changes.
    pub session: session::SessionUi,
    item_fuel_catalog: Option<Arc<assets::RuntimeEntityAssets>>,
    toast_display_millis: u64,
    /// Developer captures never draw toasts.
    hide_toasts: bool,
    chat_history: ChatHistory,
    dropped_unsent_chat_messages: u64,
    inventory_keys: interaction::InventoryKeys,
    furnace_projection: Arc<std::sync::Mutex<presentation::forms::furnace_book::ProjectionCache>>,
    recipe_book_icons: Arc<std::sync::Mutex<presentation::forms::BookIconCache>>,
    /// Startup-loaded localization catalog; survives session replacement
    /// because it is local pinned data, not server state.
    lang_catalog: Option<Arc<assets::RuntimeLangCatalog>>,
    active_lang: Option<Arc<assets::RuntimeLangCatalog>>,
    /// Version-pinned local item facts survive replacement of server session state.
    food_catalog: Option<Arc<assets::RuntimeEquipmentCatalog>>,
    stack_facts: protocol::ItemStackFactsCache,
    /// Invalidates names when components or any translation table changes.
    item_name_generation: u64,
    /// The live catalog's screen settings, which the scene stack reads.
    screen_settings: Arc<scene_stack::ScreenSettingsTable>,
    loading_screen: Option<ui::LoadingStage>,
    /// A client-owned cover shown while a global resource-pack replacement publishes.
    resource_pack_loading: bool,
}

impl UiRuntime {
    pub fn new(session_id: u64) -> Self {
        Self {
            session: session::SessionUi::new(session_id),
            item_fuel_catalog: None,
            screen_settings: Arc::default(),
            loading_screen: None,
            resource_pack_loading: false,
            toast_display_millis: ui::TOAST_DISPLAY_MILLIS,
            hide_toasts: false,
            chat_history: ChatHistory::default(),
            dropped_unsent_chat_messages: 0,
            inventory_keys: interaction::InventoryKeys::default(),
            furnace_projection: Arc::default(),
            recipe_book_icons: Arc::default(),
            lang_catalog: None,
            active_lang: None,
            food_catalog: None,
            stack_facts: Default::default(),
            item_name_generation: 0,
        }
    }

    /// Shows or hides the client-owned cover used while global packs are replaced in-session.
    pub fn set_resource_pack_loading(&mut self, loading: bool) {
        self.resource_pack_loading = loading;
    }

    /// Whether the global resource-pack replacement currently owns the screen.
    pub const fn resource_pack_loading(&self) -> bool {
        self.resource_pack_loading
    }

    pub const fn session_id(&self) -> u64 {
        self.session.session_id
    }

    pub const fn inventory_authority(
        &self,
        player_runtime: &player_state::PlayerState,
    ) -> Option<InventoryAuthority> {
        player_runtime.inventory.inventory_authority()
    }

    pub const fn local_selected_equipment<'a>(
        &self,
        player_runtime: &'a player_state::PlayerState,
    ) -> Option<&'a SequencedLocalEquipment> {
        player_runtime.inventory.local_selected_equipment()
    }

    pub fn publish_inventory_authority(
        &mut self,
        player_runtime: &mut player_state::PlayerState,
        authority: InventoryAuthority,
    ) {
        player_runtime
            .inventory
            .publish_inventory_authority(authority);
        if authority != InventoryAuthority::Server {
            self.session.inventory_open = false;
        }
    }

    /// The local player's StartGame-assigned runtime id, once known — required to address the
    /// local player in outbound packets such as the hotbar-selection `MobEquipment`.
    pub fn local_runtime_id(&self, player_runtime: &player_state::PlayerState) -> Option<u64> {
        player_runtime.inventory.local_runtime_id()
    }

    pub fn publish_local_runtime_id(
        &mut self,
        player_runtime: &mut player_state::PlayerState,
        session_id: u64,
        runtime_id: u64,
    ) -> Result<Vec<EquipmentRoute>, InventoryRouterError> {
        player_runtime
            .inventory
            .publish_local_runtime_id(session_id, runtime_id)
    }

    pub fn route_equipment(
        &mut self,
        player_runtime: &mut player_state::PlayerState,
        session_id: u64,
        fifo_sequence: u64,
        event: EquipmentEvent,
    ) -> Result<inventory::inventory_router::EquipmentRouteResult, InventoryRouterError> {
        player_runtime
            .inventory
            .route_equipment(session_id, fifo_sequence, event)
    }

    /// Retains the local player's own MobEquipment echo. Like vanilla, an offhand echo changes
    /// nothing: the local offhand is only ever read from the inventory ledger.
    pub fn retain_local_selected_equipment(
        &mut self,
        player_runtime: &mut player_state::PlayerState,
        fifo_sequence: u64,
        event: EquipmentEvent,
    ) {
        player_runtime
            .inventory
            .retain_local_selected_equipment(fifo_sequence, event);
    }

    pub const fn hud(&self) -> &HudStore {
        &self.session.hud
    }

    pub const fn chat(&self) -> &ChatStore {
        &self.session.chat
    }

    pub const fn scoreboards(&self) -> &ScoreboardStore {
        &self.session.scoreboards
    }

    pub const fn boss_bars(&self) -> &BossBarStore {
        &self.session.boss_bars
    }

    pub const fn chat_focused(&self) -> bool {
        self.session.chat_focused
    }

    pub const fn inventory_open(&self) -> bool {
        self.session.inventory_open
    }

    pub fn inventory_ledger<'a>(
        &self,
        player_runtime: &'a player_state::PlayerState,
    ) -> &'a PlayerInventoryLedger {
        player_runtime.inventory.ledger()
    }

    pub const fn sign_editor(&self) -> &sign_editor::SignEditor {
        &self.session.sign_editor
    }

    pub fn sign_editor_mut(&mut self) -> &mut sign_editor::SignEditor {
        &mut self.session.sign_editor
    }

    pub const fn server_forms(&self) -> &ServerFormStore {
        &self.session.forms
    }

    /// Answers one retained server form and stages its outbound
    /// `ModalFormResponse` for [`flush_form_response`].
    pub fn respond_to_server_form(
        &mut self,
        identity: ServerFormIdentity,
        action: LocalFormAction,
    ) -> Result<(), FormRespondError> {
        self.session.forms.respond(identity, action)
    }

    /// Answers a form and keeps its surface visible briefly for a possible server handoff.
    pub fn respond_to_server_form_at(
        &mut self,
        identity: ServerFormIdentity,
        action: LocalFormAction,
        now_millis: u64,
    ) -> Result<(), FormRespondError> {
        self.session.forms.respond_at(identity, action, now_millis)
    }

    /// Advances the visual grace period after a form answer.
    pub fn advance_form_handoff(&mut self, now_millis: u64) -> bool {
        self.session.forms.advance_form_handoff(now_millis)
    }

    pub fn server_forms_mut(&mut self) -> &mut ServerFormStore {
        &mut self.session.forms
    }

    pub fn data_driven_ui(&self) -> &data_driven::DataDrivenUi {
        &self.session.data_driven
    }

    pub fn note_stream_dimension(&mut self, dimension: i32) {
        self.session.forms.note_stream_dimension(dimension);
        self.session
            .block_cracks
            .synchronize_dimension(Some(dimension));
    }

    /// Installs validated compiled fuel facts for subsequent inventory admission.
    pub fn set_item_fuel_catalog(&mut self, catalog: Arc<assets::RuntimeEntityAssets>) {
        self.item_fuel_catalog = Some(catalog);
    }

    pub fn inventory_ledger_mut<'a>(
        &mut self,
        player_runtime: &'a mut player_state::PlayerState,
    ) -> &'a mut PlayerInventoryLedger {
        let ledger = player_runtime.inventory_commands();
        if let Some(catalog) = &self.item_fuel_catalog {
            ledger.set_item_fuel_catalog(Arc::clone(catalog));
        }
        if let Some((position, block_entity)) = &self.session.storage_block {
            ledger.name_storage_block(*position, Some(&**block_entity));
        }
        ledger
    }

    /// Keeps the screen aligned with the retained intent while timed-out controls recover.
    pub fn poll_inventory_timeout(
        &mut self,
        player_runtime: &mut player_state::PlayerState,
        now_millis: u64,
    ) {
        if player_runtime.inventory.poll_inventory_timeout(now_millis) {
            self.session.inventory_open = player_runtime
                .inventory
                .ledger()
                .personal_inventory_desired_open()
                || player_runtime
                    .inventory
                    .ledger()
                    .storage_generation()
                    .is_some();
            if !self.session.inventory_open {
                self.session.inventory_pointer_gui = None;
            }
        }
    }

    pub fn inventory_transport_closed(&mut self, player_runtime: &mut player_state::PlayerState) {
        player_runtime.inventory.ledger_mut().transport_closed();
        self.session.inventory_open = false;
        self.session.inventory_pointer_gui = None;
    }

    pub const fn inventory_pointer_gui(&self) -> Option<[f32; 2]> {
        self.session.inventory_pointer_gui
    }

    pub const fn screen_state(&self) -> &screen_state::ScreenState {
        &self.session.screen
    }

    pub fn screen_state_mut(&mut self) -> &mut screen_state::ScreenState {
        &mut self.session.screen
    }

    pub fn set_inventory_pointer_gui(&mut self, position: Option<[f32; 2]>) {
        self.session.inventory_pointer_gui = position;
    }

    /// Whether a scene over the game, menus aside, takes gameplay input.
    pub fn ui_focused(&self, player_runtime: &player_state::PlayerState) -> bool {
        !self.gameplay_input(player_runtime, None)
    }

    pub const fn chat_editor(&self) -> &ChatEditor {
        &self.session.chat_editor
    }

    pub fn chat_suggestions(&self) -> &[Arc<str>] {
        self.session.chat_autocomplete.suggestions()
    }

    pub const fn chat_selected_suggestion(&self) -> Option<usize> {
        self.session.chat_autocomplete.selected_index()
    }

    pub fn chat_usage_hint(&self) -> Option<&str> {
        self.session.chat_usage_hint.as_deref()
    }

    pub fn take_chat_autocomplete_request(&mut self) -> Option<ChatAutocompleteRequest> {
        self.session.pending_chat_autocomplete_request.take()
    }

    pub fn service_pending_chat_autocomplete(
        &mut self,
        player_runtime: &player_state::PlayerState,
    ) -> bool {
        let Some(request) = self.take_chat_autocomplete_request() else {
            return false;
        };
        self.complete_chat_autocomplete(player_runtime, request)
    }

    pub fn insert_chat_text(&mut self, value: &str) -> Result<(), ChatEditorError> {
        let before = self.session.chat_editor.clone();
        self.session.chat_editor.insert(value)?;
        if self.session.chat_editor != before {
            self.note_chat_editor_change();
        }
        Ok(())
    }

    pub fn paste_chat_text<C: ChatClipboard>(
        &mut self,
        clipboard: &mut C,
    ) -> Result<(), ChatPasteError<C::Error>> {
        let before = self.session.chat_editor.clone();
        self.session.chat_editor.paste_from(clipboard)?;
        if self.session.chat_editor != before {
            self.note_chat_editor_change();
        }
        Ok(())
    }

    pub fn move_chat_cursor_left(&mut self) {
        self.mutate_chat_editor(ChatEditor::move_left);
    }

    pub fn move_chat_cursor_right(&mut self) {
        self.mutate_chat_editor(ChatEditor::move_right);
    }

    pub fn backspace_chat_text(&mut self) {
        self.mutate_chat_editor(ChatEditor::backspace);
    }

    pub fn delete_chat_text(&mut self) {
        self.mutate_chat_editor(ChatEditor::delete);
    }

    pub fn move_chat_cursor_home(&mut self, selecting: bool) {
        self.mutate_chat_editor(|editor| editor.move_home(selecting));
    }

    pub fn move_chat_cursor_end(&mut self, selecting: bool) {
        self.mutate_chat_editor(|editor| editor.move_end(selecting));
    }

    pub fn show_older_chat_history(&mut self) -> bool {
        let Some(entry) = self.chat_history.older() else {
            return false;
        };
        self.replace_chat_editor(&entry);
        true
    }

    pub fn show_newer_chat_history(&mut self) -> bool {
        let Some(entry) = self.chat_history.newer() else {
            return false;
        };
        self.replace_chat_editor(&entry);
        true
    }

    pub fn project_block_cracks(&mut self, snapshot: client_world::BlockCrackSnapshot) {
        self.session.block_cracks.project(snapshot);
        self.session
            .block_cracks
            .report_status(self.session.session_id, self.block_cracks_status());
    }

    pub fn block_crack_snapshot(&self) -> Option<&client_world::BlockCrackSnapshot> {
        self.session.block_cracks.snapshot()
    }

    pub fn block_cracks_status(&self) -> client_world::BlockCrackStatus {
        self.session.block_cracks.status()
    }

    pub fn clear_disconnected_block_cracks(&mut self) {
        self.session.block_cracks.synchronize_dimension(None);
        self.session
            .block_cracks
            .report_status(self.session.session_id, self.block_cracks_status());
    }

    /// Resets UI session state for `session_id`; the session controller moves player authority with it.
    pub fn begin_session(&mut self, session_id: u64) {
        if self.session.session_id == session_id {
            return;
        }
        let dropped = self.session.chat_sends.pending().len();
        self.session = self.session.next_session(session_id);
        self.chat_history.clear_navigation();
        self.item_name_generation = self.item_name_generation.wrapping_add(1);
        self.dropped_unsent_chat_messages = self
            .dropped_unsent_chat_messages
            .saturating_add(dropped as u64);
    }

    pub fn open_chat(
        &mut self,
        player_runtime: &mut player_state::PlayerState,
    ) -> UiAuthorityTransition {
        player_runtime
            .inventory
            .ledger_mut()
            .request_storage_close();
        player_runtime
            .inventory
            .ledger_mut()
            .request_personal_close();
        self.session.inventory_open = false;
        self.session.chat_focused = true;
        UiAuthorityTransition {
            consumes_text: true,
            requested_context: InputContext::UiFocused,
        }
    }

    pub fn close_chat(&mut self) -> UiAuthorityTransition {
        self.session.chat_focused = false;
        self.session.chat_editor.clear();
        self.chat_history.clear_navigation();
        self.session.chat_autocomplete.clear();
        self.session.chat_usage_hint = None;
        self.session.pending_chat_autocomplete_request = None;
        UiAuthorityTransition {
            consumes_text: false,
            requested_context: InputContext::Gameplay,
        }
    }

    /// Toggles the active inventory against the actor authority available at this input edge.
    pub fn toggle_inventory(
        &mut self,
        player_runtime: &mut player_state::PlayerState,
        authority: Option<client_world::ui_read::InventoryUi<'_>>,
    ) -> UiAuthorityTransition {
        self.session.screen.container_close_armed = false;
        self.session.chat_focused = false;
        if player_runtime
            .inventory
            .ledger()
            .storage_generation()
            .is_some()
        {
            player_runtime
                .inventory
                .ledger_mut()
                .request_storage_close();
            self.session.inventory_open = false;
        } else if self.session.inventory_open {
            player_runtime
                .inventory
                .ledger_mut()
                .request_personal_close();
            self.session.inventory_open = false;
        } else {
            self.session.inventory_open =
                self.local_runtime_id(player_runtime)
                    .is_some_and(|runtime_id| {
                        let mount = authority
                            .filter(|world| world.local_player().runtime_id() == runtime_id)
                            .and_then(|world| {
                                player_runtime
                                    .facts
                                    .mount_unique_id()
                                    .and_then(|mount| world.container_actor(mount))
                            });
                        let ledger = player_runtime.inventory.ledger_mut();
                        ledger.request_personal_open(
                            mount.map_or(runtime_id, |mount| mount.runtime_id),
                        )
                    });
        }
        self.session.screen.inventory = None;
        if self.session.inventory_open {
            self.observe_inventory_window(player_runtime);
        } else {
            self.session.screen.search_focused = false;
        }
        UiAuthorityTransition {
            consumes_text: false,
            requested_context: if self.session.inventory_open {
                InputContext::UiFocused
            } else {
                InputContext::Gameplay
            },
        }
    }

    pub fn close_inventory(
        &mut self,
        player_runtime: &mut player_state::PlayerState,
    ) -> UiAuthorityTransition {
        self.session.screen.container_close_armed = false;
        self.session.screen.inventory = None;
        self.session.screen.search_focused = false;
        player_runtime
            .inventory
            .ledger_mut()
            .request_storage_close();
        player_runtime
            .inventory
            .ledger_mut()
            .request_personal_close();
        self.session.inventory_open = false;
        UiAuthorityTransition {
            consumes_text: false,
            requested_context: InputContext::Gameplay,
        }
    }

    /// Applies an editor command and advances completion state only when its contents change.
    pub fn mutate_chat_editor(&mut self, mutate: impl FnOnce(&mut ChatEditor)) {
        let before = self.session.chat_editor.clone();
        mutate(&mut self.session.chat_editor);
        if self.session.chat_editor != before {
            self.note_chat_editor_change();
        }
    }

    fn replace_chat_editor(&mut self, value: &str) {
        if self.session.chat_editor.as_str() == value
            && self.session.chat_editor.cursor_byte() == self.session.chat_editor.len_bytes()
        {
            return;
        }
        self.session.chat_editor.clear();
        self.session
            .chat_editor
            .insert(value)
            .expect("history and autocomplete entries obey the chat input bound");
        self.note_chat_editor_change();
    }

    fn note_chat_editor_change(&mut self) {
        self.session.chat_usage_hint = None;
        self.session.chat_tab_cycling = false;
        self.session.chat_tab_start = None;
        self.session.chat_input_revision = self.session.chat_input_revision.saturating_add(1);
        self.session.pending_chat_autocomplete_request = self
            .session
            .chat_autocomplete
            .begin_input(
                self.session.session_id,
                self.session.chat_input_revision,
                self.session.chat_editor.as_str(),
                self.session.chat_editor.cursor_byte(),
            )
            .expect("the editor enforces autocomplete input and cursor bounds");
    }

    pub fn retain_block_crack(
        &mut self,
        envelope: SequencedBlockCrackEvent,
    ) -> Result<(), UiRuntimeError> {
        if envelope.session_id != self.session.session_id {
            return Err(UiRuntimeError::WrongSession {
                expected: self.session.session_id,
                actual: envelope.session_id,
            });
        }
        if let Some(previous) = self.session.last_block_crack_sequence
            && envelope.fifo_sequence <= previous
        {
            return Err(UiRuntimeError::StaleBlockCrackSequence {
                previous,
                actual: envelope.fifo_sequence,
            });
        }
        self.session.last_block_crack_sequence = Some(envelope.fifo_sequence);
        Ok(())
    }

    fn validate_identity(
        &self,
        session_id: u64,
        fifo_sequence: u64,
        local_millis: u64,
        server_tick: Option<u64>,
    ) -> Result<(), UiRuntimeError> {
        if session_id != self.session.session_id {
            return Err(UiRuntimeError::WrongSession {
                expected: self.session.session_id,
                actual: session_id,
            });
        }
        if let Some(previous) = self.session.last_fifo_sequence
            && fifo_sequence <= previous
        {
            return Err(UiRuntimeError::StaleFifoSequence {
                previous,
                actual: fifo_sequence,
            });
        }
        if let Some(previous) = self.session.last_local_millis
            && local_millis < previous
        {
            return Err(UiRuntimeError::NonMonotonicLocalTime {
                previous,
                actual: local_millis,
            });
        }
        if let (Some(previous), Some(actual)) = (self.session.last_server_tick, server_tick)
            && actual != 0
            && actual < previous
        {
            return Err(UiRuntimeError::NonMonotonicServerTick { previous, actual });
        }
        Ok(())
    }
}

#[cfg(test)]
pub mod tests;

impl UiRuntime {
    /// Borrows UI gesture state for the existing app input system.
    pub fn inventory_keys_mut(&mut self) -> &mut interaction::InventoryKeys {
        &mut self.inventory_keys
    }
    /// Applies the toast preference even when this frame cannot publish a viewport.
    pub fn set_toast_lifetime(&mut self, lifetime: u64) {
        self.toast_display_millis = lifetime;
    }

    /// Stops drawing toasts for the session (developer `--hide-toasts`); they still queue
    /// and expire as usual, so nothing else changes.
    pub fn set_hide_toasts(&mut self, hide: bool) {
        self.hide_toasts = hide;
    }

    pub fn hides_toasts(&self) -> bool {
        self.hide_toasts
    }

    /// Expires retained HUD output when the app has admitted a valid viewport.
    pub fn expire_hud(&mut self, now: u64) {
        self.session.hud.expire(now);
    }
}
