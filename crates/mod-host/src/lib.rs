//! Experimental component host. Only the explicit WIT imports carry authority.

mod declarations;
mod panel_order;
mod source;
pub use source::{SourceKind, is_frame_source, is_packet_source, read_source, validate_source};
mod drawing_policy;
use drawing_policy::DrawingRestart;
mod grants;
pub use declarations::{
    Declarations, MAX_REQUESTED_GRANTS_BYTES, REQUESTED_GRANTS_SECTION, declarations,
    declarations_snapshot,
};
pub mod helper;
pub use grants::{Capability, ModGrants};
mod action_filter;
mod handlers;
pub use runtime::cinnabar::extension::sound_handling::{Position as SoundPosition, SoundContext};
pub use runtime::cinnabar::extension::teleport_handling::{
    Rotation as TeleportRotation, TeleportContext, TeleportMode,
};
mod block_search;
pub use block_search::{BlockSearch, BlockSearchSource, FoundBlocks, refresh_block_searches};
mod callback_budget;
mod callbacks;
pub use callback_budget::{CallbackBudget, CallbackOutcome};
pub use runtime::cinnabar::extension::action_filter::{
    ActionFrame, ActionVerdict, FilteredAction, Held as ActionHeld, Transition as ActionTransition,
    TransitionKind,
};
mod compiled;
mod component_assets;
mod exclusive;
mod execution;
mod load;
pub mod local_files;
pub mod module_observations;
mod outputs;
pub mod package;
pub mod packet_mod;
pub mod presentation;
mod runtime;
mod screens;
pub mod server;
mod settings;
mod terrain;
pub use settings::{SettingsSnapshot, settings_companion};

pub use packet_mod::NativeFrame;
pub use runtime::HudTextMeasure;
pub use runtime::cinnabar::extension::input::{
    ActionBinding as InputActionBinding, BindingContext, BindingModifiers,
    Bindings as InputBindings,
};
pub use runtime::legacy_input::{
    MovementSnapshot as GameplayMovementSnapshot, Snapshot as GameplaySnapshot,
};
pub use screens::{DataSource, KeyModifiers, LoadedPackage, ModEvent, ModScreens};

pub use runtime::cinnabar::extension::gameplay::{
    CameraPose as GameplayCameraPose, CameraRig as GameplayCameraRig,
    ControlModes as GameplayControlModes, Mob as GameplayMob,
    MovementInput as GameplayMovementInput, MovementIntents as GameplayMovementIntents,
    Player as GameplayPlayer, Vector3 as GameplayVector3,
};
pub use runtime::cinnabar::extension::player_state::{
    Actor as ActorObservation, ActorLife, Bounds as ActorObservationBounds,
    ChatMessage as ObservedChatMessage, CombatEvent as CombatObservation, CombatKind,
    Effect as PlayerStateEffect, Food as FoodObservation, Item as PlayerStateItem,
    Motion as MotionObservation, ObservedActor, Sidebar as SidebarSnapshot, SidebarRow,
    Slot as PlayerStateSlot, Snapshot as PlayerStateSnapshot, Vitals as VitalsObservation,
    WorldObservation, WorldTime as WorldTimeObservation,
};
pub use runtime::cinnabar::extension::{
    camera::DamageShake, events::Cue as ModCue, input::Controls as ControlFrame,
    panel::Event as ControlEvent,
};

pub use runtime::cinnabar::extension::terrain::{
    BlockState as TerrainBlockState, ChunkRevision as TerrainChunkRevision,
    Geometry as TerrainGeometry, Physics as TerrainPhysics, PhysicsFlags as TerrainPhysicsFlags,
    Position as TerrainPosition, ReadError as TerrainReadError, ReadResult as TerrainReadResult,
    Region as TerrainRegion, RegionQuery as TerrainQuery, ResolvedShape as TerrainShape,
    ShapeBinding as TerrainShapeBinding, ShapeOverride as TerrainShapeOverride,
    Stamp as TerrainStamp, SurfaceResponse as TerrainSurfaceResponse,
};
pub use terrain::{
    TerrainCheck, TerrainDescriptors, TerrainMetadata, TerrainRefresh, TerrainScheduler,
    TerrainSource, terrain_query_size, terrain_region_bytes,
};

/// Client facts the gameplay capabilities read, captured once per frame for every mod. Each
/// import still checks its own grant and gameplay frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GameplayFacts {
    /// Admitted controls before any mod outputs, only while gameplay owns input.
    pub movement_input: Option<GameplayMovementInput>,
    /// The presented camera pose, including third-person and rig offsets.
    pub camera: Option<GameplayCameraPose>,
    /// The full-strength native damage shake of the last presented view, in a connected world.
    pub damage_shake: Option<DamageShake>,
    /// The local feet position last relayed to the server, in a connected world.
    pub relayed_position: Option<GameplayVector3>,
    /// A physical binding holds the use control.
    pub use_held: bool,
    /// Current physical attack control, before mod outputs.
    pub attack_held: bool,
    /// Current crosshair block hit, before mod outputs.
    pub block_target: Option<GameplayVector3>,
}

/// Successfully committed local interaction requests, consumed once per frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct InteractionOutput {
    pub attack_reach: Option<f32>,
    pub attack_pulse: bool,
    /// Ordinary crosshair mining hold, renewed by a successful mod frame.
    pub block_mining: bool,
    /// A one-frame destination for physically held mining, independent of the view.
    pub block_mining_target: Option<GameplayVector3>,
}

/// A vanilla inventory request a player mod committed, consumed once; the client makes it as
/// the vanilla screen it stands for would, and the server checks it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InventoryRequest {
    /// Exchanges a player cell with the offhand within the observed session.
    SwapOffhand { session: u64, slot: u8 },
    /// A creative catalog take of `count` of the item `identifier` with `aux`.
    CreativeTake {
        identifier: String,
        aux: u16,
        count: u8,
    },
    /// One recipe book craft of crafting recipe `network_id`.
    FillRecipe { network_id: u32 },
}

/// Committed local actor rotation; yaw turns left and pitch turns up, in radians.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CameraDelta {
    pub yaw: f32,
    pub pitch: f32,
}
use {
    runtime::Instance,
    server_experience::{screen::ScreenLayout, session_data::SessionData},
    std::{path::PathBuf, sync::Arc},
};

/// Maximum bytes accepted before compilation or allocation of a package buffer.
pub const MAX_COMPONENT_BYTES: usize = 4 * 1024 * 1024;
/// Maximum local component envelope; executable bytes retain their separate original ceiling.
pub const MAX_COMPONENT_SOURCE_BYTES: usize = MAX_COMPONENT_BYTES
    + ui::mod_panel::MAX_PANEL_BUNDLE_BYTES
    + mod_api::MAX_PANEL_FONT_BYTES
    + mod_api::MAX_MENU_LOGO_BYTES
    + mod_api::MAX_MENU_TITLE_BYTES
    + mod_api::MAX_NOTICES_BYTES
    + mod_api::MAX_ICON_BYTES
    + component_assets::MAX_SECTION_FRAMING_BYTES;
/// Plain-text UI limit, checked before publishing any guest output.
pub const MAX_LABEL_BYTES: usize = 256;
/// Fuel one frame callback may use. On a Ryzen 9 7900X, a release host driving the
/// first-party mod's string building and JSON encoding measured 24 µs per 100k fuel
/// at the median and 90–110 µs at p99. Scaling those measurements gives about 0.24 ms
/// median and 0.9–1.1 ms p99 for this allowance, or 0.48 ms and 1.8–2.2 ms for
/// `mod_api::TOTAL_MOD_FRAME_FUEL`. These estimates are not wall-clock guarantees;
/// host imports and cache effects can take longer.
pub const FRAME_FUEL: u64 = 1_000_000;
/// One frame callback per observed world session may use this larger allowance.
pub const WORLD_ENTRY_FRAME_FUEL: u64 = 4 * FRAME_FUEL;
const _: () = assert!(
    mod_api::TOTAL_MOD_FRAME_FUEL == 2 * FRAME_FUEL,
    "a frame admits two full frame callbacks"
);
/// Maximum linear memory available to one personal mod.
pub const MEMORY_BYTES: usize = 16 * 1024 * 1024;

/// Where a mod came from, which reload reads again.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
    Component(PathBuf),
    Package(PathBuf),
}
/// A developer-selected component with transactional reload and trap quarantine.
pub struct ModHost {
    compiled: Arc<compiled::CompiledComponent>,
    instance: Instance,
    embedded_font: Option<Arc<[u8]>>,
    embedded_logo: Option<Arc<[u8]>>,
    embedded_title: Option<Arc<str>>,
    embedded_notices: Option<Arc<str>>,
    panel_bundle_override: Option<Arc<ui::mod_panel::PanelBundle>>,
    source: Source,
    attempted: [u8; 32],
    admitted_digest: [u8; 32],
    grants: ModGrants,
    /// The loader's authorization before component or manifest requests narrow it.
    authorized_grants: ModGrants,
    settings_writer: Option<settings::SettingsWriter>,
    settings_seed: Option<String>,
    pending_events: Vec<ModEvent>,
    package: Option<LoadedPackage>,
    layout: Option<ScreenLayout>,
    session: Arc<SessionData>,
    drawing_session: Option<mod_api::drawing::DrawingSession>,
    drawing_restart: DrawingRestart,
    drawing_world_active: bool,
}

/// Builds an empty, unfocused control snapshot for a frame with no routed input.
pub fn empty_controls() -> ControlFrame {
    ControlFrame {
        seconds: 0.0,
        focused: false,
        gameplay: false,
        panel_open: false,
        keys_pressed: Vec::new(),
        keys_held: Vec::new(),
        keys_admitted: Vec::new(),
        events: Vec::new(),
        pointer: None,
        wheel: 0.0,
    }
}

#[cfg(test)]
mod tests;
