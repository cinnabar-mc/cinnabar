//! UI state owned by one server session and its replacement boundary.

use std::{collections::VecDeque, sync::Arc};

use protocol::ChatAutocompleteCatalog;
use ui::{
    BossBarStore, ChatAutocompleteRequest, ChatAutocompleteState, ChatEditor, ChatRateLimit,
    ChatSendQueue, ChatStore, HudStore, MAX_CHAT_INPUT_BYTES, ScoreboardStore,
};

use super::{
    ServerFormStore, anvil, boss, credits, data_driven, emotes, gameplay_hud::GameplayHudState,
    grindstone, item_activation, item_facts, mob_effects, player_roster, presentation,
    screen_state, sign_editor, use_on_identity_evidence,
};

const MAX_PENDING_CHAT_SENDS: usize = 32;
const MAX_CHAT_SENDS_PER_WINDOW: usize = 5;
const CHAT_RATE_WINDOW_MILLIS: u64 = 2_000;

/// Server-owned presentation, input and outbound queues retired together on a server change.
#[derive(Clone, Debug)]
pub struct SessionUi {
    #[cfg(not(target_arch = "wasm32"))]
    pub experiences: crate::experience_session::ExperienceSession,
    pub player_roster: player_roster::PlayerRoster,
    pub(super) session_id: u64,
    /// The open window's position and observed block entity identity.
    pub(super) storage_block: Option<([i32; 3], Arc<str>)>,
    pub(super) last_fifo_sequence: Option<u64>,
    pub(super) last_block_crack_sequence: Option<u64>,
    pub(super) last_local_millis: Option<u64>,
    pub(super) last_server_tick: Option<u64>,
    /// Presentation tick and real millis anchor, independent of packet ordering.
    pub(super) presentation_tick_anchor: Option<(u64, u64)>,
    pub(super) chat_focused: bool,
    pub(super) inventory_open: bool,
    pub(super) hud: HudStore,
    pub(super) death_reason: Arc<str>,
    pub(super) death_rules: protocol::DeathRules,
    pub(super) chat: ChatStore,
    pub(super) scoreboards: ScoreboardStore,
    pub(super) boss_bars: BossBarStore,
    pub(super) boss_responses: boss::Responses,
    pub(super) chat_editor: ChatEditor,
    pub(super) chat_input_revision: u64,
    pub(super) chat_autocomplete: ChatAutocompleteState,
    pub(super) chat_autocomplete_catalog: ChatAutocompleteCatalog,
    pub(super) chat_usage_hint: Option<Arc<str>>,
    pub(super) local_sleeping: bool,
    pub(super) wake_requested: bool,
    pub(super) sleep_status: Option<oreui::SleepStatus>,
    pub(super) chat_tab_cycling: bool,
    pub(super) chat_tab_start: Option<usize>,
    pub(super) pending_chat_autocomplete_request: Option<ChatAutocompleteRequest>,
    pub(super) chat_sends: ChatSendQueue,
    pub(super) in_flight_chat_sends: usize,
    pub(super) chat_source_name: Arc<str>,
    pub(super) chat_xuid: Arc<str>,
    pub(super) local_player_display_name: Arc<str>,
    pub(super) block_cracks: crate::block_cracks::BlockCracks,
    pub(super) gameplay_hud: GameplayHudState,
    pub(super) item_activation: Option<item_activation::ItemActivation>,
    pub(super) pending_totem_snapshots: VecDeque<(u64, Option<u32>)>,
    pub(super) activation_clock: item_activation::ActivationClock,
    pub(super) use_on_identity_evidence: use_on_identity_evidence::UseOnIdentityEvidence,
    pub(super) forms: ServerFormStore,
    pub(super) data_driven: data_driven::DataDrivenUi,
    pub(super) credits: credits::CreditsState,
    pub(super) sign_editor: sign_editor::SignEditor,
    pub(super) inventory_pointer_gui: Option<[f32; 2]>,
    pub(super) screen: screen_state::ScreenState,
    pub(super) emotes: emotes::EmoteState,
    pub(super) mob_effect_screen: mob_effects::MobEffectScreenState,
    /// Client packets the screens queue for the network flush.
    pub(super) client_packets: VecDeque<protocol::Packet>,
    /// One complete book commit, bounded by the protocol page limit plus signing.
    pub(super) book_packets: VecDeque<protocol::Packet>,
    pub(super) local_actor_damage: Option<client_world::ActorDamageState>,
    /// Last known life state also retains the dead-to-alive reason retirement history.
    pub(super) last_known_local_player_alive: Option<bool>,
    pub(super) local_player_health_available: bool,
    pub(super) local_player_respawn_ready: bool,
    pub(super) local_player_death_generation: u64,
    pub(super) last_selected_identity_change_millis: Option<u64>,
    pub(super) last_selected_identity: Option<(u8, i32, u32)>,
    /// Local millis at which the held jump began charging the mounted jump
    /// bar; `None` while jump is released or no mount is ridden.
    pub(super) mount_jump_hold_started_millis: Option<u64>,
    pub(super) server_lang: Option<Arc<assets::ServerLangOverlay>>,
    pub(super) session_icons: Option<Arc<presentation::SessionIcons>>,
    pub(super) session_items: Option<Arc<item_facts::SessionItemComponents>>,
    pub(super) anvil_cache: anvil::cache::PreviewCache,
    pub(super) grindstone_cache: grindstone::PreviewCache,
    pub(super) server_ui: Option<Arc<presentation::ServerUiPack>>,
    pub(super) session_glyphs: Option<Arc<presentation::SessionGlyphSheets>>,
    /// Authoritative display names of real player/entity score owners,
    /// refreshed from the world stream before committed events apply.
    pub(super) score_owner_names: std::collections::BTreeMap<i64, Arc<str>>,
    /// Sorted usernames on the authoritative player list, the retained
    /// answer for the `@a` selector.
    pub(super) known_player_names: Vec<Arc<str>>,
    pub(super) player_list_held: bool,
    pub(super) touch_player_list: bool,
    pub(super) hurt_pending: bool, // a health drop the scene stack has not yet answered
}

impl SessionUi {
    /// Starts with empty server state and the configured local diagnostic policy.
    pub(super) fn new(session_id: u64) -> Self {
        Self {
            session_id,
            storage_block: None,
            #[cfg(not(target_arch = "wasm32"))]
            experiences: Default::default(),
            player_roster: Default::default(),
            last_fifo_sequence: None,
            last_block_crack_sequence: None,
            last_local_millis: None,
            last_server_tick: None,
            presentation_tick_anchor: None,
            chat_focused: false,
            inventory_open: false,
            score_owner_names: std::collections::BTreeMap::new(),
            known_player_names: Vec::new(),
            player_list_held: false,
            touch_player_list: false,
            hurt_pending: false,
            hud: HudStore::default(),
            death_reason: Arc::from(""),
            death_rules: protocol::DeathRules::default(),
            chat: ChatStore::default(),
            scoreboards: ScoreboardStore::default(),
            boss_bars: BossBarStore::default(),
            boss_responses: boss::Responses::default(),
            chat_editor: ChatEditor::new(MAX_CHAT_INPUT_BYTES)
                .expect("the reviewed chat input bound is valid"),
            chat_input_revision: 0,
            chat_autocomplete: ChatAutocompleteState::default(),
            chat_autocomplete_catalog: ChatAutocompleteCatalog::default(),
            chat_usage_hint: None,
            local_sleeping: false,
            wake_requested: false,
            sleep_status: None,
            chat_tab_cycling: false,
            chat_tab_start: None,
            pending_chat_autocomplete_request: None,
            chat_sends: ChatSendQueue::new(
                MAX_PENDING_CHAT_SENDS,
                ChatRateLimit::new(MAX_CHAT_SENDS_PER_WINDOW, CHAT_RATE_WINDOW_MILLIS)
                    .expect("the reviewed chat rate window is valid"),
            )
            .expect("the reviewed chat queue capacity is valid"),
            in_flight_chat_sends: 0,
            chat_source_name: Arc::from(""),
            chat_xuid: Arc::from(""),
            local_player_display_name: Arc::from(""),
            block_cracks: crate::block_cracks::BlockCracks::default(),
            gameplay_hud: GameplayHudState::default(),
            item_activation: None,
            pending_totem_snapshots: VecDeque::new(),
            activation_clock: item_activation::ActivationClock::default(),
            use_on_identity_evidence:
                use_on_identity_evidence::UseOnIdentityEvidence::from_environment(session_id),
            forms: ServerFormStore::default(),
            data_driven: data_driven::DataDrivenUi::default(),
            credits: credits::CreditsState::default(),
            sign_editor: sign_editor::SignEditor::default(),
            inventory_pointer_gui: None,
            screen: screen_state::ScreenState::default(),
            emotes: emotes::EmoteState::default(),
            mob_effect_screen: mob_effects::MobEffectScreenState::default(),
            client_packets: VecDeque::new(),
            book_packets: VecDeque::new(),
            local_actor_damage: None,
            last_known_local_player_alive: None,
            local_player_health_available: false,
            local_player_respawn_ready: false,
            local_player_death_generation: 0,
            last_selected_identity_change_millis: None,
            last_selected_identity: None,
            mount_jump_hold_started_millis: None,
            server_lang: None,
            session_icons: None,
            session_items: None,
            anvil_cache: Default::default(),
            grindstone_cache: Default::default(),
            server_ui: None,
            session_glyphs: None,
        }
    }

    /// Replaces server state while carrying only each owner's retained metadata and preferences.
    pub(super) fn next_session(&mut self, session_id: u64) -> Self {
        let mut next = Self::new(session_id);
        next.chat = self.chat.next_session();
        next.scoreboards = self.scoreboards.next_session();
        next.forms = self.forms.next_session();
        next.data_driven = self.data_driven.next_session();
        next.emotes = self.emotes.next_session();
        next.use_on_identity_evidence = self.use_on_identity_evidence.next_session(session_id);
        #[cfg(not(target_arch = "wasm32"))]
        {
            next.experiences = self.experiences.next_session();
        }
        next.chat_autocomplete.begin_session(session_id);
        next.chat_sends.begin_session(session_id);
        next
    }
}
