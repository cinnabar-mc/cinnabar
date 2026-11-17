//! Refreshes Bevy's private surface configuration when the requested frame latency changes.

use std::{
    num::NonZeroU32,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
    },
};

#[cfg(any(target_os = "windows", test))]
pub(crate) mod adaptive;

#[cfg(test)]
use bevy::prelude::{Result, World};
use bevy::{
    prelude::{Commands, Entity, IntoScheduleConfigs, Query, Res, ResMut, Resource, SubApp, With},
    render::{
        Extract, ExtractSchedule, Render, RenderSystems,
        view::{
            ViewTarget, ViewTargetAttachments,
            window::{ExtractedWindow, SurfaceData, create_surfaces},
        },
    },
    window::Window,
};

#[cfg(test)]
mod tests;

/// Initial surface latency: Windows waits for measured headroom and Android always queues two
/// frames; other delivery paths retain their platform default until feedback is available.
#[must_use]
pub fn frame_latency_for_vsync(vsync: bool) -> NonZeroU32 {
    initial_latency(Delivery::PLATFORM, vsync, presentation_holds_a_drawable())
}

/// How a platform delivers frames, which decides the queue depth it starts with.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Delivery {
    /// Starts with two queued frames until adaptive feedback measures headroom for one.
    Windows,
    /// The swapchain holds one image more than the latency. With one queued frame that is two
    /// images, and a frame slightly over one refresh period then waits for the next refresh to
    /// free an image, so the frame rate drops to half the refresh rate.
    Android,
    /// Follows the VSync choice and any drawable presentation holds.
    Other,
}

impl Delivery {
    const PLATFORM: Self = if cfg!(target_os = "windows") {
        Self::Windows
    } else if cfg!(target_os = "android") {
        Self::Android
    } else {
        Self::Other
    };
}

/// The latency `delivery` uses without measured feedback.
const fn initial_latency(delivery: Delivery, vsync: bool, held_drawable: bool) -> NonZeroU32 {
    match delivery {
        Delivery::Windows | Delivery::Android => NonZeroU32::new(2).expect("two is nonzero"),
        Delivery::Other => frame_latency(vsync, held_drawable),
    }
}

/// Display timing and queue depth exchanged between the window owner and the renderer.
#[derive(Clone, Debug)]
pub(super) struct FrameLatencyFeedback {
    pub(super) display: Arc<Mutex<render_model::DisplayTiming>>,
    pub(super) latency: Arc<AtomicU32>,
}

impl Default for FrameLatencyFeedback {
    fn default() -> Self {
        Self {
            display: Arc::default(),
            latency: Arc::new(AtomicU32::new(if cfg!(target_os = "windows") {
                2
            } else {
                0
            })),
        }
    }
}

/// Uses adaptive feedback only where it cannot consume a drawable held by presentation.
pub(super) fn selected_latency(vsync: bool, feedback: &FrameLatencyFeedback) -> NonZeroU32 {
    resolve_latency(
        vsync,
        presentation_holds_a_drawable(),
        feedback.latency.load(Ordering::Acquire),
    )
}

/// Applies measured feedback while retaining the slot of a drawable held by presentation.
fn resolve_latency(vsync: bool, held_drawable: bool, measured: u32) -> NonZeroU32 {
    if vsync
        && !held_drawable
        && let Some(latency) = NonZeroU32::new(measured)
    {
        latency
    } else {
        initial_latency(Delivery::PLATFORM, vsync, held_drawable)
    }
}

/// One queued frame with VSync, or two when presentation holds a drawable beyond the queue;
/// two without VSync to overlap CPU and GPU work.
const fn frame_latency(vsync: bool, held_drawable: bool) -> NonZeroU32 {
    if vsync && !held_drawable {
        NonZeroU32::MIN
    } else {
        NonZeroU32::new(2).expect("two is nonzero")
    }
}

/// Whether synchronized presentation holds one drawable beyond the queued frame. A macOS display
/// link prefetches one; iOS keeps each presented drawable until a later refresh, so with one
/// queued frame (two drawables) an iPhone presents at half its refresh rate.
fn presentation_holds_a_drawable() -> bool {
    #[cfg(target_os = "macos")]
    {
        wgpu::hal::metal::display_link_delivery_supported()
    }
    #[cfg(target_os = "ios")]
    {
        true
    }
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    {
        false
    }
}

#[derive(Resource, Default)]
struct ChangedFrameLatency(bool);

/// Installs extraction and surface invalidation before Bevy configures or acquires a frame.
pub(super) fn install(render_app: &mut SubApp) {
    #[cfg(target_os = "windows")]
    adaptive::install(render_app);
    render_app
        .init_resource::<ChangedFrameLatency>()
        .add_systems(ExtractSchedule, extract_frame_latency)
        .add_systems(
            Render,
            recreate_surfaces
                .run_if(frame_latency_changed)
                .after(RenderSystems::ExtractCommands)
                .before(create_surfaces),
        );
}

/// Keeps native surface work, including its main-thread dispatch, off unchanged frames.
fn frame_latency_changed(changed: Res<ChangedFrameLatency>) -> bool {
    changed.0
}

/// Copies live latency changes; Bevy only copies this field on the first extraction.
fn extract_frame_latency(
    windows: Extract<Query<&Window>>,
    mut extracted: Query<(bevy::render::sync_world::MainEntity, &mut ExtractedWindow)>,
    mut changed: ResMut<ChangedFrameLatency>,
) {
    for (entity, mut extracted) in &mut extracted {
        if let Ok(window) = windows.get(entity)
            && extracted.desired_maximum_frame_latency != window.desired_maximum_frame_latency
        {
            extracted.desired_maximum_frame_latency = window.desired_maximum_frame_latency;
            changed.0 = true;
        }
    }
}

/// Drops retained outputs before rebuilding the surface cache with the new frame latency.
fn recreate_surfaces(
    #[cfg(any(target_os = "macos", target_os = "ios"))] _marker: bevy::ecs::system::NonSendMarker,
    mut commands: Commands,
    mut changed: ResMut<ChangedFrameLatency>,
    mut windows: Query<(Entity, &mut ExtractedWindow)>,
    mut attachments: ResMut<ViewTargetAttachments>,
    targets: Query<Entity, With<ViewTarget>>,
    #[cfg(target_os = "windows")] adaptive: Option<ResMut<adaptive::AdaptiveFrameLatency>>,
) {
    if !std::mem::take(&mut changed.0) {
        return;
    }
    #[cfg(target_os = "windows")]
    if let Some(mut adaptive) = adaptive {
        adaptive.discard_current_frame();
    }
    for entity in &targets {
        commands.entity(entity).remove::<ViewTarget>();
    }
    attachments.clear();
    for (entity, mut window) in &mut windows {
        commands.entity(entity).remove::<SurfaceData>();
        window.swap_chain_texture_view = None;
        window.swap_chain_texture = None;
    }
}
