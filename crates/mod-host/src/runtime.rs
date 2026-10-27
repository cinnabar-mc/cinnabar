mod action_filter;
mod input_bindings;
mod input_host;
pub(crate) mod legacy_input;
mod output_lifecycle;
mod presentation;
mod recovery;
mod world_lifecycle;
use crate::{
    CallbackOutcome, CameraDelta, FRAME_FUEL, GameplayCameraRig, GameplayMob,
    GameplayMovementSnapshot, GameplaySnapshot, MAX_LABEL_BYTES, MEMORY_BYTES, ModCue, ModEvent,
    ModGrants, ModScreens, PlayerStateSnapshot,
};
use anyhow::{Result, bail};
use server_experience::{
    runtime::{CALLBACK_FUEL, LOAD_FUEL},
    screen::ScreenLayout as HostLayout,
    session_data::SessionData,
};
use std::{collections::BTreeSet, sync::Arc};
use wasmtime::{Store, StoreLimits, StoreLimitsBuilder};

// `player-mod` includes `extension`, so one linker serves both worlds' components.
wasmtime::component::bindgen!({
    path: [
        "../experience-sdk/wit/client/deps/server-experience",
        "../experience-sdk/wit/session",
        "../mod-api/wit",
    ],
    world: "cinnabar:extension/filtered-player-mod@0.1.0",
    imports: { default: trappable },
    additional_derives: [PartialEq],
    with: { "cinnabar:extension/budget@0.1.0": crate::runtime::budget_host },
});

pub(crate) const MAX_IMPORT_WRITES: u32 = 8;
/// Consecutive frame fuel overruns before a local guest loses its session authority.
pub(crate) const MAX_CONSECUTIVE_FRAME_OVERRUNS: u32 = 3;
#[path = "budget_host.rs"]
pub mod budget_host;
#[path = "camera.rs"]
mod camera;
#[path = "controls.rs"]
mod controls;
#[path = "drawing.rs"]
mod drawing;
#[cfg(test)]
mod drawing_tests;
mod exports;
#[path = "facts_host.rs"]
mod facts_host;
#[path = "gameplay.rs"]
mod gameplay;
#[path = "hud.rs"]
mod hud;
pub use hud::HudTextMeasure;
mod inventory;
#[path = "item_use.rs"]
mod item_use;
mod loading;
#[path = "masks.rs"]
mod masks;
pub(crate) mod player_mod;
#[path = "player_state.rs"]
mod player_state;
#[path = "presentation_host.rs"]
mod presentation_host;
#[path = "render.rs"]
mod render;
#[path = "terrain_host.rs"]
mod terrain_host;
#[path = "touch.rs"]
mod touch;

/// What a package declares that the host checks guest output and events against.
#[derive(Clone, Debug, Default)]
pub(crate) struct Declared {
    pub(crate) templates: BTreeSet<String>,
    pub(crate) actions: BTreeSet<String>,
    pub(crate) keys: BTreeSet<String>,
}

pub(crate) struct State {
    exclusive: crate::exclusive::Exclusive,
    terrain: crate::terrain::TerrainState,
    limits: StoreLimits,
    pressed: bool,
    label: Option<String>,
    hud: hud::HudState,
    presentation: presentation_host::PresentationState,
    facts: facts_host::FactsState,
    pending: Option<String>,
    writes: u32,
    pub(crate) grants: ModGrants,
    environment: output_lifecycle::Environment,
    motion: output_lifecycle::Movement,
    snapshot: Option<GameplaySnapshot>,
    gameplay_reads: u32,
    camera_writes: u32,
    pending_camera: Option<CameraDelta>,
    camera_delta: Option<CameraDelta>,
    packet_delay_ms: u32,
    pending_packet_delay: Option<u32>,
    packet_delay_writes: u32,
    /// Client facts delivered before this frame's callback and dropped after it.
    gameplay_facts: crate::GameplayFacts,
    controls: controls::ControlState,
    world: gameplay::WorldState,
    camera_policy: camera::CameraPolicy,
    item_use_policy: item_use::ItemUsePolicy,
    player_state: player_state::PlayerState,
    pub(crate) render: render::RenderState,
    drawing_policy: Option<mod_api::drawing::DrawingSession>,
    presentation_epoch: Option<u64>,
    pub(crate) asset_files: std::collections::BTreeMap<String, Vec<u8>>,
    declared: Declared,
    layout: Option<HostLayout>,
    session: Arc<SessionData>,
    screens: ModScreens,
    /// This callback's screen transaction, reused across writes and committed on return.
    pending_screens: Option<player_mod::ScreenTransaction>,
    /// Host calls and screen output bytes of the running callback.
    calls: usize,
    output: usize,
    inventory: inventory::InventoryRequests,
    events: player_mod::EventContext,
    touch: touch::TouchState,
}

impl State {
    /// Derives camera ownership from current transient policy and the retained rig.
    fn camera_output(&self) -> bool {
        self.world.rig.is_some()
            || self.camera_delta.is_some()
            || self.camera_policy.committed
            || self.camera_policy.committed_view_scale.is_some()
            || self.camera_policy.committed_view_tilt.is_some()
    }

    pub(crate) fn new(grants: ModGrants, settings: String, declared: Declared) -> Self {
        Self {
            limits: StoreLimitsBuilder::new()
                .memory_size(MEMORY_BYTES)
                .table_elements(4096)
                .instances(16)
                .memories(1)
                .tables(2)
                .trap_on_grow_failure(true)
                .build(),
            terrain: Default::default(),
            pressed: false,
            label: None,
            exclusive: Default::default(),
            hud: hud::HudState::default(),
            presentation: Default::default(),
            facts: Default::default(),
            pending: None,
            writes: 0,
            grants,
            environment: Default::default(),
            motion: Default::default(),
            snapshot: None,
            gameplay_reads: 0,
            camera_writes: 0,
            pending_camera: None,
            camera_delta: None,
            packet_delay_ms: 0,
            pending_packet_delay: None,
            packet_delay_writes: 0,
            gameplay_facts: Default::default(),
            controls: controls::ControlState::new(settings),
            world: gameplay::WorldState::default(),
            camera_policy: camera::CameraPolicy::default(),
            item_use_policy: item_use::ItemUsePolicy::default(),
            player_state: player_state::PlayerState::default(),
            render: render::RenderState::new(),
            drawing_policy: None,
            presentation_epoch: None,
            asset_files: Default::default(),
            declared,
            layout: None,
            session: Arc::default(),
            screens: ModScreens::default(),
            pending_screens: None,
            calls: 0,
            output: 0,
            inventory: Default::default(),
            events: Default::default(),
            touch: Default::default(),
        }
    }
}

impl cinnabar::extension::hud::Host for State {
    fn local_message(&mut self, text: String) -> Result<Result<(), String>> {
        hud::local_message(self, text)
    }
    /// Stages the mod's retained surface; it publishes when the callback succeeds.
    fn set_surface(&mut self, json: String) -> Result<Result<(), String>> {
        hud::surface::set(self, json)
    }

    /// Patches the staged or committed surface's bindings and collection order.
    fn update_surface(&mut self, json: String) -> Result<Result<(), String>> {
        hud::surface::update(self, json)
    }

    fn read_viewport(&mut self) -> Result<Option<cinnabar::extension::hud::Viewport>> {
        Ok(hud::surface::viewport(self))
    }

    fn measure_text(
        &mut self,
        text: String,
        font: cinnabar::extension::hud::Font,
    ) -> Result<Result<f32, String>> {
        Ok(hud::surface::measure(self, &text, font))
    }

    fn set_hidden_elements(
        &mut self,
        elements: Vec<cinnabar::extension::hud::VanillaElement>,
    ) -> Result<Result<(), String>> {
        hud::surface::set_hidden(self, elements)
    }

    fn read_elements(
        &mut self,
    ) -> Result<Result<Vec<cinnabar::extension::hud::ElementState>, String>> {
        Ok(hud::surface::elements(self))
    }

    /// Stages bounded plain text; nothing is published until the guest returns.
    fn set_label(&mut self, text: String) -> Result<Result<(), String>> {
        self.writes += 1;
        if self.writes > MAX_IMPORT_WRITES {
            bail!("HUD import budget exhausted");
        }
        if text.len() > MAX_LABEL_BYTES || text.chars().any(|c| c.is_control() || c == '§') {
            return Ok(Err("label must be short plain text".into()));
        }
        self.pending = Some(text);
        Ok(Ok(()))
    }
}

impl cinnabar::extension::environment::Host for State {
    /// Stages presentation weather under the same authority and budget as visual time.
    fn set_weather_override(
        &mut self,
        value: Option<cinnabar::extension::environment::Weather>,
    ) -> Result<Result<(), String>> {
        self.environment.write()?;
        if self.exclusive.environment {
            return Ok(Err(self.exclusive.refuse()));
        }
        if !self.grants.environment {
            return Ok(Err("environment capability denied".into()));
        }
        self.environment.pending_weather = Some(value);
        Ok(Ok(()))
    }

    fn set_fullbright(&mut self, enabled: bool) -> Result<Result<(), String>> {
        self.environment.write()?;
        if !self.grants.fullbright {
            return Ok(Err("fullbright capability denied".into()));
        }
        self.environment.pending_fullbright = Some(enabled);
        Ok(Ok(()))
    }

    /// Stages a fixed visual clock only with explicit authority and a valid tick.
    fn set_time_override(&mut self, ticks: Option<u32>) -> Result<Result<(), String>> {
        self.environment.write()?;
        if self.exclusive.environment {
            return Ok(Err(self.exclusive.refuse()));
        }
        if !self.grants.environment {
            return Ok(Err("environment capability denied".into()));
        }
        if ticks.is_some_and(|tick| tick >= mod_api::BEDROCK_DAY_TICKS) {
            return Ok(Err("time override must be within one Bedrock day".into()));
        }
        self.environment.pending_time = Some(ticks);
        Ok(Ok(()))
    }
}

impl cinnabar::extension::clock::Host for State {
    /// Reads the operating system's local clock at the host boundary.
    fn read_local_time(&mut self) -> Result<Result<cinnabar::extension::clock::LocalTime, String>> {
        if !self.grants.clock {
            return Ok(Err("clock capability denied".into()));
        }
        let now = chrono::Local::now();
        Ok(Ok(cinnabar::extension::clock::LocalTime {
            unix_ms: u64::try_from(now.timestamp_millis()).unwrap_or(0),
            utc_offset_seconds: now.offset().local_minus_utc(),
        }))
    }
}

/// Recoverable fuel traps retain output but cannot enter the trapped component again.
#[derive(Clone, Copy, PartialEq, Eq)]
enum InstanceStatus {
    Running,
    Recovering,
    Revoked,
    Quarantined,
}

pub(super) struct Instance {
    store: Store<State>,
    pre: wasmtime::component::InstancePre<State>,
    exports: exports::Exports,
    status: InstanceStatus,
    /// Fuel the last `init` or callback consumed.
    last_fuel: u64,
    callback_budget: Option<crate::CallbackBudget>,
    filter_identity: u64,
    consecutive_frame_overruns: u32,
}

impl Instance {
    /// Validates presentation observations before they enter the callback sandbox.
    pub(super) fn observe_presentation(
        &mut self,
        world: Option<crate::WorldObservation>,
        fps: Option<f32>,
        sidebar: Option<crate::SidebarSnapshot>,
        chat: Vec<crate::ObservedChatMessage>,
    ) -> Result<()> {
        anyhow::ensure!(
            world.as_ref().is_none_or(|w| w.session != 0
                && !w.world.is_empty()
                && w.world.len() <= mod_api::MAX_WORLD_IDENTITY_BYTES
                && !w.world.chars().any(char::is_control)
                && [w.position.0, w.position.1, w.position.2]
                    .iter()
                    .all(|v| v.is_finite())),
            "invalid world observation"
        );
        anyhow::ensure!(
            fps.is_none_or(|value| value.is_finite() && (0.0..=100000.0).contains(&value)),
            "invalid frame observation"
        );
        anyhow::ensure!(
            sidebar.as_ref().is_none_or(|s| s.rows.len() <= 15
                && s.title.len() <= ui::mod_hud::MAX_OBSERVED_TEXT_BYTES
                && s.rows
                    .iter()
                    .all(|r| r.label.len() <= ui::mod_hud::MAX_OBSERVED_TEXT_BYTES)),
            "invalid sidebar observation"
        );
        anyhow::ensure!(
            chat.len() <= ui::mod_hud::MAX_OBSERVED_CHAT_MESSAGES
                && chat
                    .iter()
                    .all(|m| m.text.len() <= ui::mod_hud::MAX_OBSERVED_TEXT_BYTES)
                && chat
                    .windows(2)
                    .all(|pair| pair[0].sequence < pair[1].sequence),
            "invalid chat observation"
        );
        let state = self.store.data_mut();
        state.player_state.world = world;
        state.player_state.fps = fps;
        state.player_state.sidebar = sidebar;
        state.player_state.chat = chat;
        Ok(())
    }

    /// Validates native cosmetic facts before exposing any current callback data.
    pub(super) fn observe_modules(
        &mut self,
        observations: crate::module_observations::ModuleObservations,
    ) -> Result<()> {
        super::module_observations::validate(&observations)?;
        self.store.data_mut().player_state.observations = observations;
        Ok(())
    }

    /// Clears transient outputs even when no guest callback can execute this frame.
    pub(super) fn begin_frame(&mut self) {
        let state = self.store.data_mut();
        state.terrain.begin_frame();
        state.snapshot = None;
        state.motion.begin_frame();
        state.pending_camera = None;
        state.camera_delta = None;
        state.pending_packet_delay = None;
        state.packet_delay_writes = 0;
        state.environment.begin_frame();
        state.controls.begin_frame();
        state.render.begin_frame();
        state.world.begin_frame();
        state.player_state.begin_frame();
        state.camera_policy = camera::CameraPolicy::default();
        state.item_use_policy = item_use::ItemUsePolicy::default();
        state.exclusive.camera_claimed = state.camera_output();
        state.pressed = false;
        state.controls.frame = crate::empty_controls();
    }

    /// Validates client facts for the next frame callback only.
    pub(super) fn observe_gameplay(&mut self, facts: crate::GameplayFacts) -> Result<()> {
        gameplay::validate_facts(&facts)?;
        self.store.data_mut().gameplay_facts = facts;
        Ok(())
    }

    /// Releases callback-only observations on every executed, deferred or failed frame path.
    pub(super) fn finish_frame(&mut self) {
        let state = self.store.data_mut();
        state.snapshot = None;
        state.gameplay_facts = Default::default();
        state.player_state.begin_frame();
        state.player_state.clear_presentation();
        state.motion.finish_frame();
        state.world.mobs.clear();
        state.world.incoming.clear();
        state.controls.frame = crate::empty_controls();
        state.pressed = false;
    }

    /// Validates and stages current observations even when lifecycle delivery defers.
    pub(super) fn observe_frame(
        &mut self,
        pressed: bool,
        snapshot: Option<GameplaySnapshot>,
        mobs: Vec<GameplayMob>,
        player_state: Option<PlayerStateSnapshot>,
        movement: Option<GameplayMovementSnapshot>,
        controls: crate::ControlFrame,
    ) -> Result<()> {
        // Deferred events may stage frame-only output; only this callback may publish it.
        self.begin_frame();
        if !self.is_active() {
            self.store.data_mut().player_state.revoke();
            return Ok(());
        }
        let validation = gameplay::validate_snapshot(snapshot.as_ref())
            .and_then(|()| gameplay::validate_mobs(snapshot.as_ref(), &mobs))
            .and_then(|()| gameplay::validate_movement(snapshot.as_ref(), movement.as_ref()))
            .and_then(|()| player_state::validate(player_state.as_ref()))
            .and_then(|()| controls::validate_frame(&controls));
        if let Err(error) = validation {
            self.store.data_mut().player_state.revoke();
            return Err(error);
        }
        let snapshot_seconds = snapshot.as_ref().map_or(0.0, |frame| frame.frame_seconds);
        let mut controls = controls;
        if !controls.panel_open {
            // Pointer and wheel belong to the mod whose open panel owns input.
            controls.pointer = None;
            controls.wheel = 0.0;
        }
        let state = self.store.data_mut();
        state.pressed = pressed;
        state.snapshot = snapshot;
        state.motion.snapshot = movement;
        state.world.advance_command_window(snapshot_seconds);
        state.world.mobs = mobs;
        state.player_state.set_snapshot(player_state)?;
        state.controls.frame = controls;
        Ok(())
    }

    /// Refreshes native presentation on deferred delivery and publishes guest output on success.
    pub(super) fn frame(&mut self, delivery: CallbackOutcome) -> Result<()> {
        if !self.is_active() {
            return Ok(());
        }
        self.store.data_mut().controls.begin_text_frame();
        self.store.data_mut().touch.begin_frame();
        let result = match delivery {
            CallbackOutcome::Executed => {
                self.run(FRAME_FUEL, commit, |exports, store| exports.frame(store))
            }
            CallbackOutcome::Deferred => Ok(CallbackOutcome::Deferred),
        };
        let deferred = matches!(&result, Ok(CallbackOutcome::Deferred));
        let state = self.store.data_mut();
        state.controls.finish_text_frame(deferred);
        state.touch.finish_frame(deferred);
        result.map(|_| ())
    }

    /// Checks event authority and payload bounds before admitting deferred state.
    pub(super) fn validate_event(&self, event: &ModEvent) -> Result<()> {
        self.store.data().check_event(event)
    }

    /// Delivers one event callback, `data-changed` with `LOAD_FUEL` and every other event with
    /// `CALLBACK_FUEL`. An `extension` component has no event exports and receives nothing.
    pub(super) fn dispatch(&mut self, event: &ModEvent) -> Result<CallbackOutcome> {
        if !self.is_active() {
            return Ok(CallbackOutcome::Executed);
        }
        self.validate_event(event)?;
        if !self.exports.handles_event(event) {
            return Ok(CallbackOutcome::Executed);
        }
        let fuel = match event {
            ModEvent::DataChanged(_) => LOAD_FUEL,
            _ => CALLBACK_FUEL,
        };
        self.run(fuel, commit_event, |exports, store| {
            exports.event(store, event)
        })
    }

    /// Publishes only successful callbacks; frame fuel traps may retain output for recovery.
    fn run(
        &mut self,
        fuel: u64,
        publish: fn(&mut Store<State>),
        call: impl FnOnce(&exports::Exports, &mut Store<State>) -> Result<()>,
    ) -> Result<CallbackOutcome> {
        if self.status == InstanceStatus::Recovering {
            self.last_fuel = 0;
            return Ok(CallbackOutcome::Deferred);
        }
        let allowance = self
            .callback_budget
            .as_ref()
            .map_or(Some(fuel), |budget| budget.reserve(fuel));
        let Some(allowance) = allowance else {
            self.last_fuel = 0;
            return Ok(CallbackOutcome::Deferred);
        };
        let state = self.store.data_mut();
        state.writes = 0;
        state.environment.begin_callback();
        state.gameplay_reads = 0;
        state.camera_writes = 0;
        state.motion.begin_callback();
        state.packet_delay_writes = 0;
        state.controls.reset_budget();
        state.calls = 0;
        state.output = 0;
        state.pending_screens = None;
        state.inventory.begin();
        state.hud.begin_callback();
        state.render.begin_callback();
        state.touch.begin_callback();
        state.presentation.begin();
        state.facts.begin();
        crate::execution::arm(&mut self.store, allowance, fuel == LOAD_FUEL)?;
        let called = call(&self.exports, &mut self.store);
        self.last_fuel = allowance - self.store.get_fuel().unwrap_or(0).min(allowance);
        if fuel <= FRAME_FUEL
            && let Some(budget) = &self.callback_budget
        {
            budget.refund(allowance - self.last_fuel);
        }
        if let Err(error) = called {
            return Err(self.callback_failure(fuel, error));
        }
        publish(&mut self.store);
        if fuel == FRAME_FUEL {
            self.consecutive_frame_overruns = 0;
        }
        Ok(CallbackOutcome::Executed)
    }

    /// Attaches aggregate fuel and precedence for all callbacks until the next frame.
    pub(super) fn set_frame_authority(
        &mut self,
        budget: crate::CallbackBudget,
        camera: bool,
        environment: bool,
        owner: usize,
    ) {
        self.callback_budget = Some(budget);
        let state = self.store.data_mut();
        if camera && !state.exclusive.camera && state.exclusive.camera_claimed {
            state.exclusive.refuse();
        }
        if environment
            && !state.exclusive.environment
            && (state.environment.time_override.is_some()
                || state.environment.weather_override.is_some())
        {
            state.exclusive.refuse();
        }
        state.exclusive.camera_claimed |= state.camera_output();
        state.exclusive.camera = camera;
        state.exclusive.environment = environment;
        state.exclusive.owner = owner;
    }

    /// Remembers camera output even after its one-shot delta has been consumed by the app.
    pub(super) fn has_camera_output(&self) -> bool {
        let state = self.store.data();
        !state.exclusive.camera && state.exclusive.camera_claimed
    }

    /// Counts host import refusals across successful and failed callbacks.
    pub(super) fn refused_outputs(&self) -> u64 {
        self.store.data().exclusive.refused
    }

    /// Whether the mod retains authority, including while awaiting a fresh component.
    pub(super) fn is_active(&self) -> bool {
        matches!(
            self.status,
            InstanceStatus::Running | InstanceStatus::Recovering
        )
    }

    /// Whether an authority transition stopped a previously running guest.
    pub(super) fn is_revoked(&self) -> bool {
        self.status == InstanceStatus::Revoked
    }

    /// Whether callbacks must wait for a fresh guest after a frame fuel trap.
    pub(super) fn is_recovering(&self) -> bool {
        self.status == InstanceStatus::Recovering
    }

    /// Revokes a running guest without reclassifying an existing trap as recoverable.
    pub(super) fn revoke(&mut self) {
        if self.is_active() {
            self.quarantine();
            self.status = InstanceStatus::Revoked;
        }
    }

    /// Disables callbacks and drops everything the guest presented or staged.
    pub(super) fn quarantine(&mut self) {
        self.status = InstanceStatus::Quarantined;
        let state = self.store.data_mut();
        state.exclusive.camera_claimed = false;
        state.pending = None;
        state.label = None;
        state.hud.revoke();
        state.presentation = Default::default();
        state.player_state.revoke();
        state.terrain = Default::default();
        state.environment.revoke();
        state.snapshot = None;
        state.motion.revoke();
        state.pending_camera = None;
        state.camera_delta = None;
        state.controls.revoke();
        state.render.revoke();
        state.world = gameplay::WorldState::default();
        state.camera_policy = camera::CameraPolicy::default();
        state.item_use_policy = item_use::ItemUsePolicy::default();
        state.packet_delay_ms = 0;
        state.pending_packet_delay = None;
        state.gameplay_facts = Default::default();
        state.pending_screens = None;
        state.screens = ModScreens::default();
        state.inventory.revoke();
        state.touch.revoke();
    }

    /// Reads the committed use pulse before output collection, without consuming it.
    pub(super) fn use_pulse_requested(&self) -> bool {
        self.is_active() && self.store.data().item_use_policy.pulse
    }

    pub(super) fn preserves_teleport_rotation(&self) -> bool {
        !self.store.data().exclusive.camera && self.store.data().camera_policy.committed
    }

    /// Only a successful, focused gameplay callback publishes a view multiplier.
    pub(super) fn camera_view_scale(&self) -> Option<[f32; 2]> {
        (!self.store.data().exclusive.camera)
            .then_some(self.store.data().camera_policy.committed_view_scale)
            .flatten()
    }

    /// Successful frame-local presentation remains independent of gameplay input ownership.
    pub(super) fn camera_view_tilt(&self) -> Option<mod_api::native_camera::ViewTilt> {
        (!self.store.data().exclusive.camera)
            .then_some(self.store.data().camera_policy.committed_view_tilt)
            .flatten()
    }

    /// Consumes the committed cancellation once.
    pub(super) fn take_jump_cancel(&mut self) -> bool {
        std::mem::take(&mut self.store.data_mut().motion.jump_cancel)
    }

    /// Consumes the committed jump request once.
    pub(super) fn take_jump_pulse(&mut self) -> bool {
        std::mem::take(&mut self.store.data_mut().motion.jump_pulse)
    }

    /// Answers each subscribed block search through the client's terrain.
    pub(super) fn refresh_block_searches(&mut self, source: &mut impl crate::BlockSearchSource) {
        for (search, sample) in &mut self.store.data_mut().terrain.searches {
            *sample = Some(source.find(search));
        }
    }

    pub(super) fn terrain_subscriptions(&self) -> usize {
        self.store.data().terrain.subscriptions.len()
    }
    pub(super) fn refresh_terrain(
        &mut self,
        index: usize,
        refresh: impl FnOnce(
            &crate::TerrainQuery,
            Option<&crate::TerrainRegion>,
        ) -> crate::TerrainRefresh,
    ) {
        self.store.data_mut().terrain.refresh(index, refresh);
    }

    pub(super) fn camera_rig(&self) -> Option<GameplayCameraRig> {
        (!self.store.data().exclusive.camera)
            .then_some(self.store.data().world.rig)
            .flatten()
    }

    pub(super) fn take_commands(&mut self) -> Vec<String> {
        std::mem::take(&mut self.store.data_mut().world.commands)
    }

    /// Cues the next callback may poll; they last exactly one callback.
    pub(super) fn deliver_cues(&mut self, cues: Vec<ModCue>) {
        self.store.data_mut().world.incoming = gameplay::incoming(cues);
    }

    pub(super) fn take_cues(&mut self) -> Vec<ModCue> {
        std::mem::take(&mut self.store.data_mut().world.cues)
    }

    pub(super) fn take_camera_delta(&mut self) -> Option<CameraDelta> {
        let blocked = self.store.data().exclusive.camera;
        self.store
            .data_mut()
            .camera_delta
            .take()
            .filter(|_| !blocked)
    }

    pub(super) fn packet_delay_ms(&self) -> u32 {
        self.store.data().packet_delay_ms
    }
    pub(super) fn fullbright(&self) -> bool {
        self.store.data().environment.fullbright
    }

    /// Reads the committed presentation clock without entering the component.
    pub(super) fn time_override(&self) -> Option<u32> {
        (!self.store.data().exclusive.environment)
            .then_some(self.store.data().environment.time_override)
            .flatten()
    }

    /// Reads committed weather subject to active environment precedence.
    pub(super) fn weather_override(&self) -> Option<cinnabar::extension::environment::Weather> {
        (!self.store.data().exclusive.environment)
            .then_some(self.store.data().environment.weather_override)
            .flatten()
    }

    /// Reads retained UI without entering the component.
    pub(super) fn label(&self) -> Option<&str> {
        self.store.data().label.as_deref()
    }

    /// The committed HUD surface and its process-unique revision.
    pub(super) fn hud_surface(&self) -> Option<(&Arc<ui::mod_panel::Surface>, u64)> {
        let hud = &self.store.data().hud;
        Some((hud.surface.as_ref()?, hud.surface_revision))
    }

    /// Vanilla elements the committed output hides.
    pub(super) fn hidden_hud_elements(&self) -> ui::mod_hud::HiddenElements {
        self.store.data().hud.hidden
    }

    /// Records the last laid-out HUD root and vanilla element states for later reads.
    pub(super) fn observe_hud(
        &mut self,
        viewport: Option<ui::mod_hud::HudViewport>,
        elements: ui::mod_hud::ElementStates,
    ) {
        let hud = &mut self.store.data_mut().hud;
        hud.viewport = viewport.filter(|viewport| viewport.valid());
        hud.elements = elements.map(ui::mod_hud::ElementState::sanitized);
    }

    /// Records where the host last laid out this mod's open panel, for `panel.read-layout`.
    pub(super) fn observe_panel_layout(&mut self, layout: Option<ui::mod_hud::PanelLayout>) {
        self.store.data_mut().controls.layout = layout.filter(|layout| layout.valid());
    }

    /// Installs the label metrics `measure-text` reads.
    pub(super) fn set_text_measure(&mut self, text: Option<Arc<dyn hud::HudTextMeasure>>) {
        self.store.data_mut().hud.text = text;
    }

    /// The committed presentation parameters and their process-unique revision.
    pub(super) fn presentation(&self) -> (&ui::mod_presentation::ModPresentation, u64) {
        self.store.data().presentation.output()
    }

    /// Consumes committed local-only system feedback once.
    pub(super) fn take_local_messages(&mut self) -> Vec<String> {
        std::mem::take(&mut self.store.data_mut().hud.local_messages)
    }

    pub(super) fn panel(&self) -> Option<&ui::mod_panel::Panel> {
        self.store.data().controls.panel.as_ref()
    }
    /// Installs a validated local catalog outside guest callbacks.
    pub(super) fn set_panel_bundle(
        &mut self,
        bundle: Arc<ui::mod_panel::PanelBundle>,
    ) -> Result<()> {
        anyhow::ensure!(
            self.is_active() && self.store.data().grants.controls,
            "controls capability denied"
        );
        let controls = &mut self.store.data_mut().controls;
        controls.panel = Some(bundle.panel.clone());
        controls.bundle = Some(bundle);
        Ok(())
    }
    /// Keeps the local catalog available when a component reload creates a fresh instance.
    pub(super) fn panel_bundle(&self) -> Option<Arc<ui::mod_panel::PanelBundle>> {
        self.store.data().controls.bundle.clone()
    }
    pub(super) fn panel_open(&self) -> bool {
        self.store.data().controls.open
    }
    pub(super) fn set_panel_open(&mut self, open: bool) {
        let active = self.is_active();
        let state = self.store.data_mut();
        state.controls.open =
            open && active && state.grants.controls && state.controls.panel.is_some();
    }
    pub(super) fn reserved_keys(&self) -> &[String] {
        &self.store.data().controls.keys
    }
    pub(super) fn take_interaction(&mut self) -> crate::InteractionOutput {
        std::mem::take(&mut self.store.data_mut().controls.interaction)
    }
    pub(super) fn settings_write(&self) -> Option<&str> {
        self.store.data().controls.dirty_settings.as_deref()
    }
    pub(super) fn settings_written(&mut self) {
        self.store.data_mut().controls.dirty_settings = None;
    }

    pub(super) fn render(&self) -> (&mod_render::RenderOutput, u64) {
        let render = &self.store.data().render;
        (render.output(), render.generation())
    }

    pub(super) fn settings(&self) -> &str {
        self.store.data().controls.settings()
    }

    pub(super) fn screens(&self) -> &ModScreens {
        &self.store.data().screens
    }

    /// Whether the component exports `player-mod`'s event callbacks.
    pub(super) fn has_events(&self) -> bool {
        self.exports.has_events()
    }

    /// The open container screen's layout that `screen.layout` returns; none also closes the
    /// view, as closing the container returns from it.
    pub(super) fn set_layout(&mut self, layout: Option<HostLayout>) {
        let state = self.store.data_mut();
        if layout.is_none() && state.screens.view.is_some() {
            state.screens.view = None;
            state.screens.data.revision += 1;
        }
        state.layout = layout;
    }

    /// Closes the view without the guest, as Escape does; `true` when one was open.
    pub(super) fn close_view(&mut self) -> bool {
        let screens = &mut self.store.data_mut().screens;
        let open = screens.view.take().is_some();
        if open {
            screens.data.revision += 1;
        }
        open
    }

    pub(super) fn view_open(&self) -> bool {
        self.store.data().screens.view.is_some()
    }

    pub(super) fn last_fuel(&self) -> u64 {
        self.last_fuel
    }

    /// Returns only the movement intents committed by the current successful frame.
    pub(super) fn movement_intents(&self) -> Option<crate::GameplayMovementIntents> {
        self.store.data().world.movement_intents
    }

    /// Returns only the control modes committed by the current successful frame.
    pub(super) fn control_modes(&self) -> Option<crate::GameplayControlModes> {
        self.store.data().world.control_modes
    }

    pub(super) fn set_session(&mut self, session: Arc<SessionData>) {
        self.store.data_mut().session = session;
    }

    /// Installs the installation's food table that `facts.read-item` and `facts.read-foods`
    /// report.
    pub(super) fn set_food_catalog(&mut self, food: Option<Arc<assets::RuntimeEquipmentCatalog>>) {
        self.store.data_mut().facts.set_food(food);
    }

    /// The installation's food table, which a replacement instance inherits.
    pub(super) fn food_catalog(&self) -> Option<Arc<assets::RuntimeEquipmentCatalog>> {
        self.store.data().facts.food()
    }
}

/// Publishes retained presentation changes after the entire callback succeeds.
fn commit(store: &mut Store<State>) {
    let state = store.data_mut();
    state.motion.commit_frame();
    state.terrain.commit();
    // Retained drawing is committed once, by `commit_event`, so frame groups survive.
    state.render.commit_primitives();
    state.world.commit();
    state.camera_policy.committed = state.camera_policy.pending && state.snapshot.is_some();
    state.camera_policy.committed_view_scale = (state.snapshot.is_some()
        && state.controls.frame.focused
        && !state.controls.frame.panel_open)
        .then_some(state.camera_policy.pending_view_scale)
        .flatten();
    state.camera_policy.committed_view_tilt = state.camera_policy.pending_view_tilt;
    state.item_use_policy.commit(state.snapshot.is_some());
    state.camera_delta = state.pending_camera.take();
    state.exclusive.camera_claimed = state.camera_output();
    if let Some(delay) = state.pending_packet_delay.take() {
        state.packet_delay_ms = delay;
    }
    commit_event(store);
}

/// Publishes retained drawing, label, visual time, panel, settings and screen changes.
/// Immediate primitives, camera and command output belong to `frame` alone;
/// an event's staged changes to those outputs are dropped by the next frame.
fn commit_event(store: &mut Store<State>) {
    let state = store.data_mut();
    if !state.world_output_available() {
        state.clear_world_output();
    }
    state.render.commit_retained();
    state.hud.commit();
    state.presentation.commit();
    state.controls.commit();
    state.touch.commit();
    state.inventory.commit();
    state.environment.commit();
    if let Some(text) = state.pending.take() {
        state.label = (!text.is_empty()).then_some(text);
    }
    if let Some(screens) = state
        .pending_screens
        .take()
        .and_then(player_mod::ScreenTransaction::commit)
    {
        state.screens = screens;
    }
}

#[cfg(test)]
mod epoch_tests;
