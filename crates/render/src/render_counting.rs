//! GPU API requests that also count toward the render frame's work counters. Each method
//! takes and returns what the request it wraps does.

use bevy::render::{
    render_phase::TrackedRenderPass,
    render_resource::{ComputePipeline, RenderPipeline},
    renderer::{RenderContext, RenderDevice},
};
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

/// Pipelines created directly on a device. Only creations on the render thread reach a
/// frame's counters.
pub(crate) trait CountedPipelines {
    type RenderPipeline;
    type ComputePipeline;

    /// `create_render_pipeline`, counted as one pipeline.
    fn counted_render_pipeline(
        &self,
        desc: &wgpu::RenderPipelineDescriptor<'_>,
    ) -> Self::RenderPipeline;

    /// `create_compute_pipeline`, counted as one pipeline.
    fn counted_compute_pipeline(
        &self,
        desc: &wgpu::ComputePipelineDescriptor<'_>,
    ) -> Self::ComputePipeline;
}

impl CountedPipelines for wgpu::Device {
    type RenderPipeline = wgpu::RenderPipeline;
    type ComputePipeline = wgpu::ComputePipeline;

    #[inline]
    fn counted_render_pipeline(
        &self,
        desc: &wgpu::RenderPipelineDescriptor<'_>,
    ) -> wgpu::RenderPipeline {
        count(RenderCounter::Pipelines, 1);
        self.create_render_pipeline(desc)
    }

    #[inline]
    fn counted_compute_pipeline(
        &self,
        desc: &wgpu::ComputePipelineDescriptor<'_>,
    ) -> wgpu::ComputePipeline {
        count(RenderCounter::Pipelines, 1);
        self.create_compute_pipeline(desc)
    }
}

impl CountedPipelines for RenderDevice {
    type RenderPipeline = RenderPipeline;
    type ComputePipeline = ComputePipeline;

    #[inline]
    fn counted_render_pipeline(&self, desc: &wgpu::RenderPipelineDescriptor<'_>) -> RenderPipeline {
        count(RenderCounter::Pipelines, 1);
        self.create_render_pipeline(desc)
    }

    #[inline]
    fn counted_compute_pipeline(
        &self,
        desc: &wgpu::ComputePipelineDescriptor<'_>,
    ) -> ComputePipeline {
        count(RenderCounter::Pipelines, 1);
        self.create_compute_pipeline(desc)
    }
}

/// Shader modules created directly on a device. Only creations on the render thread reach a
/// frame's counters.
pub(crate) trait CountedShaders {
    /// `create_shader_module`, counted as one shader module.
    fn counted_shader_module(&self, desc: wgpu::ShaderModuleDescriptor<'_>) -> wgpu::ShaderModule;
}

impl CountedShaders for wgpu::Device {
    #[inline]
    fn counted_shader_module(&self, desc: wgpu::ShaderModuleDescriptor<'_>) -> wgpu::ShaderModule {
        count(RenderCounter::Shaders, 1);
        self.create_shader_module(desc)
    }
}

/// Validated shader modules created through the render device.
pub(crate) trait CountedValidatedShaders {
    /// `create_and_validate_shader_module`, counted as one shader module.
    fn counted_validated_shader_module(
        &self,
        desc: wgpu::ShaderModuleDescriptor<'_>,
    ) -> wgpu::ShaderModule;
}

impl CountedValidatedShaders for RenderDevice {
    #[inline]
    fn counted_validated_shader_module(
        &self,
        desc: wgpu::ShaderModuleDescriptor<'_>,
    ) -> wgpu::ShaderModule {
        count(RenderCounter::Shaders, 1);
        self.create_and_validate_shader_module(desc)
    }
}
