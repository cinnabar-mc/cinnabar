//! Several local mods in one frame. Load order resolves every conflict.

use std::path::{Path, PathBuf};

use super::packet_relay::PacketModSelection;

use serde::Deserialize;
use {
    mod_api::MAX_LOADED_MODS,
    mod_host::{
        CameraDelta, ControlEvent, ControlFrame, GameplayCameraRig, GameplayControlModes,
        GameplayMob, GameplayMovementIntents, GameplayMovementSnapshot, GameplaySnapshot, ModCue,
        ModGrants, ModHost, PlayerStateSnapshot,
    },
};

/// Loaded personal components and their merged labels, in load order.
pub struct ModHosts {
    /// Always contains at least one host, in load order.
    hosts: Vec<ModHost>,
    panel_selection: usize,
    budget: mod_host::CallbackBudget,
    label: Option<String>,
    label_inputs: Vec<String>,
    label_rebuilds: u64,
    presentation_sources: Vec<u64>,
    presentation: std::sync::Arc<ui::mod_presentation::Presentation>,
    /// Label metrics every host's `measure-text` reads.
    label_metrics: Option<std::sync::Arc<LabelWidths>>,
}

/// Measures `measure-text` widths with the presenter's label metrics.
struct LabelWidths(std::sync::Arc<mod_ui::LabelMetrics>);

impl mod_host::HudTextMeasure for LabelWidths {
    fn width(&self, text: &str, font: ui::mod_panel::LabelFont) -> Option<f32> {
        self.0.width(text, font)
    }
}

/// Shares the cached label measurer with a host without allocating another adapter.
fn label_widths(
    metrics: &Option<std::sync::Arc<LabelWidths>>,
) -> Option<std::sync::Arc<dyn mod_host::HudTextMeasure>> {
    metrics.clone().map(|metrics| metrics as _)
}

/// Selects an ordered set of components and packages, each with its own grants.
pub const SET_ENV: &str = "CINNABAR_MOD_SET";
const MAX_SET_BYTES: usize = 16 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModSet {
    version: u32,
    mods: Vec<SetEntry>,
}

/// One set entry names a bare component or a package directory, never both.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetEntry {
    #[serde(default)]
    component: Option<PathBuf>,
    #[serde(default)]
    package: Option<PathBuf>,
    #[serde(default)]
    grants: ModGrants,
}

/// Where a set entry's mod comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModSource {
    Component(PathBuf),
    /// A package's permissions need both its manifest's ask and the entry's grant.
    Package(PathBuf),
}

impl ModSource {
    /// Returns the component or package path selected by this entry.
    pub fn path(&self) -> &Path {
        match self {
            Self::Component(path) | Self::Package(path) => path,
        }
    }

    /// Loads this entry with only its explicitly supplied grants.
    pub fn load(&self, grants: ModGrants) -> anyhow::Result<ModHost> {
        match self {
            Self::Component(path) => ModHost::load_with_grants(path, grants),
            Self::Package(dir) => ModHost::load_package_with_grants(dir, grants),
        }
    }
}

/// Reads a bounded set file; its order is the load order.
pub fn read_set(path: &Path) -> Result<Vec<(ModSource, ModGrants)>, String> {
    let bytes = mod_host::local_files::read(path, MAX_SET_BYTES)
        .map_err(|error| format!("read {}: {error}", path.display()))?;
    let set: ModSet =
        serde_json::from_slice(&bytes).map_err(|error| format!("invalid mod set: {error}"))?;
    if set.version != 1 {
        return Err("unsupported mod set version".into());
    }
    if set.mods.is_empty() || set.mods.len() > MAX_LOADED_MODS {
        return Err(format!("a mod set lists 1 to {MAX_LOADED_MODS} components"));
    }
    set.mods
        .into_iter()
        .map(|entry| {
            let source = match (entry.component, entry.package) {
                (Some(path), None) => ModSource::Component(path),
                (None, Some(dir)) => ModSource::Package(dir),
                _ => return Err("a mod set entry names one component or one package".into()),
            };
            if !source.path().is_absolute() {
                return Err(format!(
                    "mod set entry {} must be absolute",
                    source.path().display()
                ));
            }
            Ok((source, entry.grants))
        })
        .collect()
}

/// The set's packet mod: its first component entry with the packets grant. Later ones are
/// ignored, since a session relays through at most one.
pub fn packet_mod_entry(mods: &[(ModSource, ModGrants)]) -> Option<PacketModSelection> {
    let mut entries = mods.iter().filter(|(_, grants)| grants.packets);
    let selected = entries.find_map(|(source, grants)| match source {
        ModSource::Component(path) => Some(PacketModSelection {
            path: path.clone(),
            grants: grants.clone(),
        }),
        ModSource::Package(_) => None,
    });
    for (source, _) in entries {
        let path = source.path().display();
        tracing::warn!(
            "Cinnabar packet mod {path} ignored: a session relays through one packet mod"
        );
    }
    selected
}

/// Loads valid entries in order and reports failures without discarding the other entries.
/// Packet mods are left to the session's relay.
pub fn load_entries(mods: Vec<(ModSource, ModGrants)>) -> Vec<ModHost> {
    mods.into_iter()
        .filter(|(_, grants)| !grants.packets)
        .filter_map(|(source, grants)| {
            source
                .load(grants)
                .inspect_err(|error| {
                    let path = source.path().display();
                    bevy::log::warn!("Cinnabar extension {path} disabled: {error:#}");
                })
                .ok()
        })
        .collect()
}

impl ModHosts {
    /// Keeps a nonempty collection of hosts in their original load order.
    pub fn new(host: ModHost, remaining: Vec<ModHost>) -> Self {
        Self {
            hosts: std::iter::once(host).chain(remaining).collect(),
            panel_selection: 0,
            budget: mod_host::CallbackBudget::new(mod_api::TOTAL_MOD_FRAME_FUEL),
            label: None,
            label_inputs: Vec::new(),
            label_rebuilds: 0,
            presentation_sources: Vec::new(),
            presentation: Default::default(),
            label_metrics: None,
        }
    }

    /// Resets the shared frame, event and loading allowances, and grants
    /// exclusive camera and time output to the first active instance publishing that output.
    pub fn begin_frame(&mut self) {
        self.budget.reset(mod_api::TOTAL_MOD_FRAME_FUEL);
        for index in 0..self.host_count() {
            self.host_mut(index).begin_frame();
        }
        for index in 0..self.host_count() {
            self.authorize(index);
        }
    }

    /// Refreshes precedence before a callback, allowing a later mod to follow an earlier trap.
    fn authorize(&mut self, index: usize) {
        let camera = (0..index).any(|earlier| {
            self.host(earlier).is_active() && self.host(earlier).has_camera_output()
        });
        let environment = (0..index).any(|earlier| {
            self.host(earlier).is_active()
                && (self.host(earlier).time_override().is_some()
                    || self.host(earlier).weather_override().is_some())
        });
        let budget = self.budget.clone();
        let host = self.host_mut(index);
        if host.is_active() {
            host.set_frame_authority(budget, camera, environment, index);
        }
    }

    /// Exchanges two hosts while keeping the selected panel attached to its instance.
    pub fn swap(&mut self, left: usize, right: usize) {
        if left == right {
            return;
        }
        self.hosts.swap(left, right);
        if self.panel_selection == left {
            self.panel_selection = right;
        } else if self.panel_selection == right {
            self.panel_selection = left;
        }
    }

    /// Removes any host while another remains, preserving the surviving order.
    pub fn remove(&mut self, index: usize) -> ModHost {
        assert!(
            self.hosts.len() > 1,
            "a mod host collection must stay nonempty"
        );
        let host = self.hosts.remove(index);
        if self.panel_selection == index {
            self.panel_selection = 0;
        } else if self.panel_selection > index {
            self.panel_selection -= 1;
        }
        host
    }

    /// Appends a host at the lowest load precedence without changing panel selection.
    pub fn push(&mut self, mut host: ModHost) {
        host.set_text_measure(label_widths(&self.label_metrics));
        self.hosts.push(host);
    }

    /// Records the last painted HUD root, its vanilla element states and its label metrics
    /// for every host's next HUD reads.
    pub fn observe_hud(
        &mut self,
        viewport: Option<ui::mod_hud::HudViewport>,
        elements: ui::mod_hud::ElementStates,
        label_metrics: Option<std::sync::Arc<mod_ui::LabelMetrics>>,
    ) {
        let changed = match (&self.label_metrics, &label_metrics) {
            (Some(old), Some(new)) => !std::sync::Arc::ptr_eq(&old.0, new),
            (None, None) => false,
            _ => true,
        };
        if changed {
            self.label_metrics =
                label_metrics.map(|metrics| std::sync::Arc::new(LabelWidths(metrics)));
        }
        let measure = label_widths(&self.label_metrics);
        for host in &mut self.hosts {
            host.observe_hud(viewport, elements);
            host.set_text_measure(measure.clone());
        }
    }

    /// Vanilla HUD elements any active host hides.
    pub fn hidden_hud_elements(&self) -> ui::mod_hud::HiddenElements {
        self.hosts.iter().fold(Default::default(), |hidden, host| {
            hidden.union(host.hidden_hud_elements())
        })
    }

    /// Consumes a collection containing exactly one host when its runtime is removed.
    pub fn into_single(mut self) -> ModHost {
        assert_eq!(
            self.hosts.len(),
            1,
            "other mod hosts still belong to this runtime"
        );
        self.hosts.pop().expect("nonempty mod hosts")
    }

    /// Borrows every host in load order without allowing collection changes.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut ModHost> {
        self.hosts.iter_mut()
    }

    /// Every active mod's presentation merged in load order, rebuilt only when a committed
    /// presentation or the set of contributing mods changes.
    pub fn merged_presentation(&mut self) -> std::sync::Arc<ui::mod_presentation::Presentation> {
        let sources = || {
            self.hosts
                .iter()
                .filter(|host| host.is_active() && !host.presentation().is_empty())
        };
        if !sources()
            .map(ModHost::presentation_revision)
            .eq(self.presentation_sources.iter().copied())
        {
            self.presentation_sources = sources().map(ModHost::presentation_revision).collect();
            self.presentation = std::sync::Arc::new(ui::mod_presentation::Presentation::merge(
                sources().map(|host| {
                    let grants = host.grants();
                    (
                        ui::mod_presentation::ActorGrants {
                            players: grants.players,
                            entities: grants.entities,
                        },
                        host.presentation(),
                    )
                }),
            ));
        }
        std::sync::Arc::clone(&self.presentation)
    }

    /// The earliest active mod with chat actions owns every clickable span and its clicks.
    pub fn chat_action_owner(&self) -> Option<usize> {
        (0..self.host_count()).find(|&index| {
            let host = self.host(index);
            host.is_active() && !host.presentation().chat_actions.is_empty()
        })
    }

    /// Supplies the installation's food table to every loaded mod's `facts` reads.
    pub fn set_food_catalog(
        &mut self,
        catalog: Option<&std::sync::Arc<assets::RuntimeEquipmentCatalog>>,
    ) {
        for host in &mut self.hosts {
            host.set_food_catalog(catalog);
        }
    }

    /// Counts every loaded component.
    pub fn host_count(&self) -> usize {
        self.hosts.len()
    }

    /// Borrows a component in load order.
    pub fn host(&self, index: usize) -> &ModHost {
        &self.hosts[index]
    }

    /// Borrows a component in load order for one callback or host action.
    pub fn host_mut(&mut self, index: usize) -> &mut ModHost {
        &mut self.hosts[index]
    }

    /// The selected panel publisher owns the retained panel and its events.
    pub fn panel_owner(&self) -> usize {
        if self.panel_selection < self.host_count()
            && self.host(self.panel_selection).panel().is_some()
        {
            return self.panel_selection;
        }
        (0..self.host_count())
            .find(|&index| self.host(index).panel().is_some())
            .unwrap_or(0)
    }

    /// Selects the earliest active panel whose toggle key was pressed, closing the old panel.
    pub fn select_panel(&mut self, pressed: &[String]) -> bool {
        let current = self.host(self.panel_owner());
        if current.panel_open() && current.panel().is_some_and(|panel| panel.capture_key) {
            return false;
        }
        let selected = (0..self.host_count()).find(|&index| {
            let host = self.host(index);
            host.is_active()
                && host
                    .panel()
                    .is_some_and(|panel| pressed.contains(&panel.toggle_key))
        });
        let Some(selected) = selected else {
            return false;
        };
        let previous = self.panel_owner();
        self.panel_selection = selected;
        if selected != previous {
            self.host_mut(previous).set_panel_open(false);
            return true;
        }
        false
    }

    /// Only the earliest active, granted, explicitly exporting component filters actions.
    pub fn action_filter_owner(&self) -> Option<usize> {
        (0..self.host_count()).find(|&i| self.host(i).action_filter_identity().is_some())
    }

    /// The mod whose package draws beside the container screens and receives their input.
    pub fn screen_owner(&self) -> Option<usize> {
        screen_owner((0..self.host_count()).map(|index| {
            let host = self.host(index);
            let screens = host.screens();
            (
                host.package().is_some() && host.is_active() && host.grants().screen,
                screens.overlay.is_some() || screens.view.is_some(),
            )
        }))
    }

    /// Every mod's label in load order, joined and cut to the plain-text limit.
    pub fn merged_label(&mut self) -> Option<&str> {
        if self.host_count() == 1 {
            return self.host(0).label();
        }
        let mut current = (0..self.host_count()).filter_map(|index| self.host(index).label());
        let unchanged = self
            .label_inputs
            .iter()
            .map(String::as_str)
            .eq(&mut current);
        if !unchanged {
            self.label_inputs = (0..self.host_count())
                .filter_map(|index| self.host(index).label().map(str::to_owned))
                .collect();
            self.label = join_labels(&self.label_inputs);
            self.label_rebuilds += 1;
        }
        self.label.as_deref()
    }

    /// Every mod's render output with its generation, in load order.
    pub fn render_outputs(&self) -> impl Iterator<Item = (&mod_render::RenderOutput, u64)> {
        (0..self.host_count()).map(|index| self.host(index).render())
    }

    /// Presented touch buttons with their owning host index, in load order. A button id an
    /// earlier mod already uses is skipped, and the set is bounded across all mods.
    pub fn touch_buttons(&self) -> Vec<(usize, &ui::mod_touch::TouchButton)> {
        let mut buttons: Vec<(usize, &ui::mod_touch::TouchButton)> = Vec::new();
        for index in 0..self.host_count() {
            for button in self.host(index).touch_buttons() {
                if buttons.len() < ui::mod_touch::MAX_PRESENTED_TOUCH_BUTTONS
                    && !buttons.iter().any(|(_, other)| other.id == button.id)
                {
                    buttons.push((index, button));
                }
            }
        }
        buttons
    }

    /// Queues a completed tap for the instance `owner` that still presents button `id`; a tap
    /// on a button now presented by another instance is dropped.
    pub fn queue_touch_press(&mut self, owner: u64, id: &str) -> bool {
        let index = self
            .touch_buttons()
            .into_iter()
            .find_map(|(index, button)| {
                (button.id == id && self.host(index).touch_owner() == owner).then_some(index)
            });
        index.is_some_and(|index| self.host_mut(index).queue_touch_press(id))
    }

    /// Shows the panel of each mod whose last executed frame received taps and opened it.
    pub fn select_panels_opened_by_taps(&mut self) {
        for index in 0..self.host_count() {
            if self.host(index).touch_presses_delivered() {
                self.select_opened_panel(index);
            }
        }
    }

    /// Shows the panel a tapped mod opened, as its toggle key would, closing the selected
    /// panel. A panel capturing keys keeps the selection.
    pub fn select_opened_panel(&mut self, index: usize) -> bool {
        let previous = self.panel_owner();
        if index >= self.host_count() || index == previous {
            return false;
        }
        let host = self.host(index);
        let current = self.host(previous);
        if !(host.is_active() && host.panel().is_some() && host.panel_open())
            || (current.panel_open() && current.panel().is_some_and(|panel| panel.capture_key))
        {
            return false;
        }
        self.panel_selection = index;
        self.host_mut(previous).set_panel_open(false);
        true
    }

    /// Collects the distinct physical keys reserved by all loaded components.
    pub fn reserved_keys(&self) -> Vec<String> {
        let mut keys: Vec<_> = (0..self.host_count())
            .flat_map(|index| self.host(index).reserved_keys().iter().cloned())
            .collect();
        keys.sort();
        keys.dedup();
        keys
    }
}

/// The controls one mod sees: keys reserved by an earlier mod are withheld, only the panel
/// owner receives panel events, and only the chat-action owner receives chat clicks.
fn claim_controls(
    frame: &ControlFrame,
    claimed: &[String],
    panel_owner: bool,
    chat_events: &[ControlEvent],
) -> ControlFrame {
    let free = |keys: &[String]| -> Vec<String> {
        keys.iter()
            .filter(|key| !claimed.contains(key))
            .cloned()
            .collect()
    };
    ControlFrame {
        seconds: frame.seconds,
        focused: frame.focused,
        gameplay: frame.gameplay,
        panel_open: frame.panel_open,
        keys_pressed: free(&frame.keys_pressed),
        keys_held: free(&frame.keys_held),
        keys_admitted: frame.keys_admitted.clone(),
        events: frame
            .events
            .iter()
            .filter(|_| panel_owner)
            .chain(chat_events)
            .take(ui::mod_panel::MAX_PANEL_EVENTS)
            .cloned()
            .collect(),
        pointer: frame.pointer.filter(|_| panel_owner),
        wheel: if panel_owner { frame.wheel } else { 0.0 },
    }
}

/// Host-owned input shared by every mod in one frame.
pub struct FrameInput<'a> {
    pub pressed: bool,
    pub controls: &'a ControlFrame,
    /// Every mod's committed cues from the previous frame.
    pub previous_cues: &'a [ModCue],
    /// Captured once from the current local session, delivered only to granted components.
    pub player_state: Option<&'a PlayerStateSnapshot>,
    pub world_observation: Option<&'a mod_host::WorldObservation>,
    pub modules: Option<&'a mod_host::module_observations::ModuleObservations>,
    pub fps: Option<f32>,
    pub sidebar: Option<&'a mod_host::SidebarSnapshot>,
    pub chat: &'a [mod_host::ObservedChatMessage],
    /// Clicked chat-action spans, reported only to [`ModHosts::chat_action_owner`].
    pub chat_events: &'a [ControlEvent],
    /// Camera, relayed position and use-control facts; each import checks its own grant.
    pub gameplay_facts: mod_host::GameplayFacts,
}

/// Runs each mod once in load order with its own grants and merges what they commit.
/// A failing mod is reported and quarantined by its host; the others still run.
pub fn run_frame(
    runtime: &mut ModHosts,
    input: FrameInput<'_>,
    world: impl FnMut(
        &ModGrants,
    ) -> (
        Option<GameplaySnapshot>,
        Vec<GameplayMob>,
        Option<GameplayMovementSnapshot>,
    ),
    failed: impl FnMut(usize, String),
) -> Merged {
    run_frame_callbacks(runtime, input, world, failed);
    collect_frame_outputs(runtime)
}

/// Runs observations and guest frames before input filtering and output collection.
pub fn run_frame_callbacks(
    runtime: &mut ModHosts,
    input: FrameInput<'_>,
    mut world: impl FnMut(
        &ModGrants,
    ) -> (
        Option<GameplaySnapshot>,
        Vec<GameplayMob>,
        Option<GameplayMovementSnapshot>,
    ),
    mut failed: impl FnMut(usize, String),
) {
    let mut union = ModGrants::default();
    let mut active = false;
    for index in 0..runtime.host_count() {
        let host = runtime.host(index);
        if !host.is_active() {
            continue;
        }
        active = true;
        let grants = host.grants();
        union.terrain |= grants.terrain;
        union.players |= grants.players;
        union.camera |= grants.camera;
        union.item_use |= grants.item_use;
        union.movement |= grants.movement;
        union.interaction |= grants.interaction;
        union.entities |= grants.entities;
        union.commands.extend(grants.commands.iter().cloned());
    }
    let (snapshot, mobs, movement) = if active {
        world(&union)
    } else {
        (None, Vec::new(), None)
    };
    let owner = runtime.panel_owner();
    let chat_owner = runtime.chat_action_owner();
    let mut claimed = Vec::new();
    for index in 0..runtime.host_count() {
        let host = runtime.host(index);
        if !host.is_active() {
            continue;
        }
        let grants = host.grants();
        let snapshot = snapshot
            .as_ref()
            .filter(|_| needs_gameplay(grants))
            .map(|frame| GameplaySnapshot {
                players: if grants.players {
                    frame.players.clone()
                } else {
                    Vec::new()
                },
                session: frame.session,
                dimension: frame.dimension,
                eye: frame.eye,
                yaw: frame.yaw,
                pitch: frame.pitch,
                frame_seconds: frame.frame_seconds,
                attack_held: frame.attack_held,
            });
        let mobs = if grants.entities {
            mobs.clone()
        } else {
            Vec::new()
        };
        let movement = if grants.movement { movement } else { None };
        let chat_events = if chat_owner == Some(index) {
            input.chat_events
        } else {
            &[]
        };
        let mut controls = claim_controls(input.controls, &claimed, index == owner, chat_events);
        controls.panel_open = input.controls.panel_open && index == owner;
        controls.gameplay = snapshot.is_some() || movement.is_some();
        claimed.extend(runtime.host(index).reserved_keys().iter().cloned());
        runtime.authorize(index);
        let host = runtime.host_mut(index);
        host.deliver_cues(input.previous_cues.to_vec());
        let player_state = host
            .grants()
            .player_state
            .then(|| input.player_state.cloned())
            .flatten();
        let observed = host.observe_presentation(
            input.world_observation.cloned(),
            input.fps,
            input.sidebar.cloned(),
            input.chat.to_vec(),
        );
        let observed = observed
            .and_then(|()| host.observe_modules(input.modules.cloned().unwrap_or_default()))
            .and_then(|()| host.observe_gameplay(input.gameplay_facts));
        if let Err(error) = observed {
            failed(index, format!("{error:#}"));
            continue;
        }
        if let Err(error) = host.frame_with_movement(
            input.pressed,
            snapshot,
            mobs,
            player_state,
            movement,
            controls,
        ) {
            failed(index, format!("{error:#}"));
        }
        if let Some(error) = host.take_settings_error() {
            bevy::log::warn!("Cinnabar extension preferences could not be saved: {error}");
        }
    }
}

/// Collects only still-active guests, so a failed input callback cannot publish prior output.
pub fn collect_frame_outputs(runtime: &mut ModHosts) -> Merged {
    let mut merged = Merged::default();
    for host in runtime.iter_mut().filter(|host| host.is_active()) {
        merged.absorb(host);
    }
    merged
}

/// Whether the host may receive the common gameplay pose and input snapshot.
fn needs_gameplay(grants: &ModGrants) -> bool {
    grants.terrain
        || grants.players
        || grants.camera
        || grants.item_use
        || grants.movement
        || grants.interaction
        || grants.entities
        || !grants.commands.is_empty()
}

/// One frame's combined output; for single-valued outputs the earliest mod wins.
#[derive(Default)]
pub struct Merged {
    pub rig: Option<GameplayCameraRig>,
    pub preserve_teleport_rotation: bool,
    pub view_scale: Option<[f32; 2]>,
    pub view_tilt: Option<mod_api::native_camera::ViewTilt>,
    pub delta: Option<CameraDelta>,
    pub time_override: Option<u32>,
    pub weather_override: Option<[f32; 2]>,
    pub attack_reach: Option<f32>,
    pub attack_pulse: bool,
    pub jump_pulse: bool,
    pub jump_cancel: bool,
    /// Every mod's sprint and sneak intents combined; any mod holding one holds it.
    pub movement_intents: Option<GameplayMovementIntents>,
    /// Every mod's control modes combined as intents are; any mod turning one on turns it on.
    pub control_modes: Option<GameplayControlModes>,
    pub commands: Vec<String>,
    pub cues: Vec<ModCue>,
}

impl Merged {
    /// Consumes `host`'s committed output, in load order.
    pub fn absorb(&mut self, host: &mut ModHost) {
        self.jump_pulse |= host.take_jump_pulse();
        self.jump_cancel |= host.take_jump_cancel();
        if let Some(intents) = host.movement_intents() {
            let merged = self
                .movement_intents
                .get_or_insert(GameplayMovementIntents {
                    sprint: false,
                    sneak: false,
                });
            merged.sprint |= intents.sprint;
            merged.sneak |= intents.sneak;
        }
        if let Some(modes) = host.control_modes() {
            let merged = self.control_modes.get_or_insert(GameplayControlModes {
                toggle_sprint: false,
                toggle_sneak: false,
                always_sprint: false,
            });
            merged.toggle_sprint |= modes.toggle_sprint;
            merged.toggle_sneak |= modes.toggle_sneak;
            merged.always_sprint |= modes.always_sprint;
        }
        let interaction = host.take_interaction();
        self.attack_reach = self.attack_reach.or(interaction.attack_reach);
        self.attack_pulse |= interaction.attack_pulse;
        let delta = host.take_camera_delta();
        self.delta = self.delta.or(delta);
        self.rig = self.rig.or(host.camera_rig());
        self.preserve_teleport_rotation |= host.preserves_teleport_rotation();
        self.view_scale = self.view_scale.or(host.camera_view_scale());
        self.view_tilt = self.view_tilt.or(host.camera_view_tilt());
        self.time_override = self.time_override.or(host.time_override());
        self.weather_override = self.weather_override.or(host.weather_override());
        self.commands.extend(host.take_commands());
        self.cues.extend(host.take_cues());
    }
}

/// Of mods in load order, each `(may draw, draws)`: the earliest that draws an overlay or view,
/// else the earliest that may draw one.
fn screen_owner(mods: impl Iterator<Item = (bool, bool)> + Clone) -> Option<usize> {
    let eligible = mods.enumerate().filter(|(_, (may, _))| *may);
    eligible
        .clone()
        .find(|(_, (_, draws))| *draws)
        .or_else(|| eligible.clone().next())
        .map(|(index, _)| index)
}

/// Joins labels with a separator, cut at a char boundary within the plain-text limit.
fn join_labels(labels: &[String]) -> Option<String> {
    if labels.is_empty() {
        return None;
    }
    let mut label = labels.join(" | ");
    if label.len() > mod_host::MAX_LABEL_BYTES {
        let mut end = mod_host::MAX_LABEL_BYTES;
        while !label.is_char_boundary(end) {
            end -= 1;
        }
        label.truncate(end);
    }
    Some(label)
}

#[cfg(test)]
#[path = "multi_tests.rs"]
mod tests;
