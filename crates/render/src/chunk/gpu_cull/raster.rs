//! Experimental current-frame box tests against opaque depth, without a depth pyramid.

use super::{kernels::cull_shader_source, model::group_count, storage::CullStorage};

pub(super) const RASTER_ENV: &str = "CINNABAR_TERRAIN_RASTER_OCCLUSION";

/// Requires forced early depth so failed samples cannot publish fragment storage writes.
pub(super) fn supported(
    backend: wgpu::Backend,
    features: wgpu::Features,
    flags: wgpu::DownlevelFlags,
) -> bool {
    backend == wgpu::Backend::Vulkan
        && features.contains(wgpu::Features::SHADER_EARLY_DEPTH_TEST)
        && flags.contains(
            wgpu::DownlevelFlags::FRAGMENT_WRITABLE_STORAGE | wgpu::DownlevelFlags::VERTEX_STORAGE,
        )
}

/// Reuses the cull shader's record layout and Rust-owned constants for both raster stages.
fn shader_source(workgroup: u32) -> String {
    let cull = cull_shader_source(workgroup);
    let (declarations, _) = cull
        .split_once("@group(0)")
        .expect("cull bindings follow types");
    format!("{declarations}\n{}", include_str!("raster.wgsl"))
}

/// Pipelines and bindings for one storage allocation and depth attachment format/sample count.
pub(super) struct RasterCull {
    pub(super) format: wgpu::TextureFormat,
    pub(super) samples: u32,
    prepare: wgpu::ComputePipeline,
    pipeline: wgpu::RenderPipeline,
    prepare_group: wgpu::BindGroup,
    group: wgpu::BindGroup,
    workgroup: u32,
}

impl RasterCull {
    /// Creates matching multisample coverage, preserving depth and every colour attachment.
    pub(super) fn new(
        device: &wgpu::Device,
        storage: &CullStorage,
        workgroup: u32,
        format: wgpu::TextureFormat,
        samples: u32,
    ) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("terrain raster occlusion"),
            source: wgpu::ShaderSource::Wgsl(shader_source(workgroup).into()),
        });
        let prepare = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("terrain raster reset"),
            layout: None,
            module: &module,
            entry_point: Some("raster_prepare"),
            compilation_options: Default::default(),
            cache: None,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("terrain raster boxes"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("raster_box"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("raster_seen"),
                compilation_options: Default::default(),
                targets: &[],
            }),
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: samples,
                ..Default::default()
            },
            multiview_mask: None,
            cache: None,
        });
        let group = |layout: &wgpu::BindGroupLayout, bindings: &[(u32, &wgpu::Buffer)]| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("terrain raster bindings"),
                layout,
                entries: &bindings
                    .iter()
                    .map(|&(binding, buffer)| wgpu::BindGroupEntry {
                        binding,
                        resource: buffer.as_entire_binding(),
                    })
                    .collect::<Vec<_>>(),
            })
        };
        let bindings = [
            (0, &storage.uniforms[1]),
            (1, &storage.records),
            (3, &storage.history),
        ];
        Self {
            prepare_group: group(&prepare.get_bind_group_layout(0), &bindings),
            group: group(&pipeline.get_bind_group_layout(0), &bindings),
            prepare,
            pipeline,
            format,
            samples,
            workgroup,
        }
    }

    /// Resets current-frame results, then sets visibility for any passing depth sample.
    pub(super) fn encode(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        depth: &wgpu::TextureView,
        slots: u32,
        viewport: [f32; 4],
        timestamps: (
            Option<wgpu::ComputePassTimestampWrites<'_>>,
            Option<wgpu::RenderPassTimestampWrites<'_>>,
        ),
    ) {
        if slots == 0 {
            return;
        }
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("terrain raster reset"),
                timestamp_writes: timestamps.0,
            });
            pass.set_pipeline(&self.prepare);
            pass.set_bind_group(0, &self.prepare_group, &[]);
            pass.dispatch_workgroups(group_count(slots, self.workgroup), 1, 1);
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("terrain raster occlusion"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: None,
                stencil_ops: None,
            }),
            timestamp_writes: timestamps.1,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.group, &[]);
        pass.set_viewport(viewport[0], viewport[1], viewport[2], viewport[3], 0.0, 1.0);
        pass.draw(0..36, 0..slots);
    }
}

#[cfg(test)]
#[path = "raster_tests.rs"]
mod tests;
