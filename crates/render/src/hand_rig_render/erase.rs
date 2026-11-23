//! Returns the private hand depth to its clear value after each draw, so the next frame loads
//! the attachment instead of clearing every multisampled texel.
//!
//! The erase repeats the frame's draw through the same vertex shader, whose position is
//! `@invariant`, with depth testing forced to pass, colour writes off and a viewport depth range
//! of `[CLEARED_DEPTH, CLEARED_DEPTH]`. Its fragment stage never discards, so it covers every
//! sample the draw could have written and returns it to the clear value; any other sample it
//! covers already held that value.

use super::*;
use bevy::render::{
    render_phase::TrackedRenderPass,
    render_resource::{LoadOp, Operations, RenderPipeline, StoreOp},
};
use std::sync::atomic::{AtomicBool, Ordering};

/// Whether an erase is cheaper than a clear on `backend`. Desktop Vulkan and DirectX 12 GPUs
/// render to memory, where a clear writes every texel. Tile-based GPUs, behind Metal, Android
/// and browsers, clear in tile memory and would instead pay to load and store the attachment.
pub(super) fn admitted(backend: wgpu::Backend) -> bool {
    matches!(backend, wgpu::Backend::Vulkan | wgpu::Backend::Dx12) && !cfg!(target_os = "android")
}

/// Reverse-Z far plane, the value the hand's depth attachment starts every draw with.
pub(super) const CLEARED_DEPTH: f32 = 0.0;

/// Whether a hand depth attachment holds only [`CLEARED_DEPTH`].
#[derive(Default)]
pub(super) struct DepthCleared(AtomicBool);

impl DepthCleared {
    /// Depth operations for a pass that ends with an erase when `erases`: an attachment already
    /// at the clear value is loaded, and only an erased one is worth storing.
    pub(super) fn operations(&self, erases: bool) -> Operations<f32> {
        Operations {
            load: if self.0.load(Ordering::Relaxed) {
                LoadOp::Load
            } else {
                LoadOp::Clear(CLEARED_DEPTH)
            },
            store: if erases {
                StoreOp::Store
            } else {
                StoreOp::Discard
            },
        }
    }

    /// Records whether the pass just encoded left the attachment at the clear value.
    pub(super) fn record(&self, erased: bool) {
        self.0.store(erased, Ordering::Relaxed);
    }
}

/// Fragment entry point of the erase: no discards and no colour.
pub(super) const ERASE_FRAGMENT: &str = "hand_depth_erase_fragment";

/// The draw pipeline with depth forced to pass and write, colour writes disabled and the
/// discard-free erase fragment. The vertex stage, primitive and multisample state stay
/// identical, so it covers every sample the draw covers.
pub(super) fn erase_pipeline(mut descriptor: RenderPipelineDescriptor) -> RenderPipelineDescriptor {
    descriptor.label = Some("first-person rig depth erase".into());
    if let Some(depth) = descriptor.depth_stencil.as_mut() {
        depth.depth_compare = Some(CompareFunction::Always);
        depth.depth_write_enabled = Some(true);
    }
    if let Some(fragment) = descriptor.fragment.as_mut() {
        fragment.entry_point = Some(ERASE_FRAGMENT.into());
        for target in fragment.targets.iter_mut().flatten() {
            target.write_mask = ColorWrites::empty();
        }
    }
    descriptor
}

/// Repeats the pass's last draw with `pipeline`, writing [`CLEARED_DEPTH`] over everything it
/// covered. The pass's bind groups stay bound; `extent` is its attachment size.
pub(super) fn erase_draw<'w>(
    pass: &mut TrackedRenderPass<'w>,
    pipeline: &'w RenderPipeline,
    extent: Extent3d,
    vertices: std::ops::Range<u32>,
    instances: std::ops::Range<u32>,
) {
    pass.set_render_pipeline(pipeline);
    pass.set_viewport(
        0.0,
        0.0,
        extent.width as f32,
        extent.height as f32,
        CLEARED_DEPTH,
        CLEARED_DEPTH,
    );
    pass.draw(vertices, instances);
}

#[cfg(test)]
#[path = "erase_tests.rs"]
mod tests;
