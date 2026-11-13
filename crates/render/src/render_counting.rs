//! Pass requests that also count toward the render frame's work counters. Each method takes
//! and returns what the request it wraps does. The architecture gate rejects production code
//! that begins a pass any other way.

use bevy::render::{render_phase::TrackedRenderPass, renderer::RenderContext};
use render_scene::runtime_profile::render_counters::{RenderCounter, count};

/// Passes begun on a command encoder.
pub(crate) trait CountedPasses {
    /// `begin_render_pass`, counted as one pass.
    fn counted_render_pass(
        &mut self,
        desc: &wgpu::RenderPassDescriptor<'_>,
    ) -> wgpu::RenderPass<'_>;

    /// `begin_compute_pass`, counted as one pass.
    fn counted_compute_pass(
        &mut self,
        desc: &wgpu::ComputePassDescriptor<'_>,
    ) -> wgpu::ComputePass<'_>;
}

impl CountedPasses for wgpu::CommandEncoder {
    #[inline]
    fn counted_render_pass(
        &mut self,
        desc: &wgpu::RenderPassDescriptor<'_>,
    ) -> wgpu::RenderPass<'_> {
        count(RenderCounter::Passes, 1);
        self.begin_render_pass(desc)
    }

    #[inline]
    fn counted_compute_pass(
        &mut self,
        desc: &wgpu::ComputePassDescriptor<'_>,
    ) -> wgpu::ComputePass<'_> {
        count(RenderCounter::Passes, 1);
        self.begin_compute_pass(desc)
    }
}

/// Tracked passes begun on a render context.
pub(crate) trait CountedTrackedPass {
    /// `begin_tracked_render_pass`, counted as one pass.
    fn counted_tracked_render_pass(
        &mut self,
        descriptor: wgpu::RenderPassDescriptor<'_>,
    ) -> TrackedRenderPass<'_>;
}

impl CountedTrackedPass for RenderContext<'_, '_> {
    #[inline]
    fn counted_tracked_render_pass(
        &mut self,
        descriptor: wgpu::RenderPassDescriptor<'_>,
    ) -> TrackedRenderPass<'_> {
        count(RenderCounter::Passes, 1);
        self.begin_tracked_render_pass(descriptor)
    }
}
