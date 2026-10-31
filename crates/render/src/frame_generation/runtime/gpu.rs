//! Prepared render storage and overlay composition. Construction runs on a finite worker.
use super::*;
use crate::scene_sampling::ResolvedDepth;
use bevy::render::{render_resource::TextureViewId, renderer::RenderDevice};

/// Every GPU property that requires a new context; camera motion never changes this key.
#[derive(Clone, PartialEq)]
pub(super) struct Key {
    pub device: wgpu::Device,
    pub size: FrameGenerationSize,
    pub samples: u32,
    pub output: wgpu::TextureFormat,
    pub depth: TextureViewId,
}

/// World-sized transparent storage for an unwarped foreground category.
pub(super) struct Layer {
    pub _texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub resolved: Option<(wgpu::Texture, wgpu::TextureView)>,
}
impl Layer {
    /// Allocates matching coverage samples and an optional single-sample colour resolve.
    fn new(device: &wgpu::Device, size: [u32; 2], samples: u32, hand: bool) -> Self {
        let format = if hand {
            render_scene::SCENE_COLOR_FORMAT
        } else {
            wgpu::TextureFormat::Rgba8Unorm
        };
        let texture = image(
            device,
            size,
            format,
            samples,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        );
        let view = texture.create_view(&Default::default());
        let resolved = (hand && samples > 1).then(|| {
            let texture = image(
                device,
                size,
                format,
                1,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            );
            let view = texture.create_view(&Default::default());
            (texture, view)
        });
        Self {
            _texture: texture,
            view,
            resolved,
        }
    }
    /// Foreground passes load a layer cleared once at the start of this real frame.
    pub fn attachment(&self) -> wgpu::RenderPassColorAttachment<'_> {
        wgpu::RenderPassColorAttachment {
            view: &self.view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
            },
        }
    }
    /// Returns native colour samples after the real frame's foreground draws finish.
    fn sampled(&self) -> &wgpu::TextureView {
        self.resolved.as_ref().map_or(&self.view, |(_, view)| view)
    }
}

/// All pipelines, history and output images for one camera configuration.
pub(super) struct Ready {
    pub key: Key,
    pub temporal: std::sync::Mutex<Temporal>,
    pub world: wgpu::Texture,
    pub _real: wgpu::Texture,
    pub real_view: wgpu::TextureView,
    pub hand: Layer,
    pub labels: Layer,
    pub depth: ResolvedDepth,
    pub overlay: wgpu::RenderPipeline,
    pub copy_generated: wgpu::RenderPipeline,
    pub copy_native: wgpu::RenderPipeline,
    pub present: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
}

/// Mutable camera and colour history, separate from immutable attachment handles.
pub(super) struct Temporal {
    pub generator: FrameGenerator,
    pub history: Option<crate::motion_blur::history::CameraHistory>,
    pub current: Option<crate::motion_blur::history::CameraHistory>,
    pub world_captured: bool,
    pub real_captured: bool,
    pub reset: bool,
}

/// Allocates a nonempty native or world image with explicit consumer usages.
fn image(
    device: &wgpu::Device,
    size: [u32; 2],
    format: wgpu::TextureFormat,
    samples: u32,
    usage: wgpu::TextureUsages,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("frame generation image"),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: samples,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    })
}

impl Ready {
    /// Prepares every pipeline and size-dependent image off the render thread.
    pub fn new(
        device: RenderDevice,
        key: Key,
        depth_source: bevy::render::render_resource::TextureView,
    ) -> Option<Self> {
        let raw = device.wgpu_device();
        let validation = raw.push_error_scope(wgpu::ErrorFilter::Validation);
        let allocation = raw.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let result = (|| {
            let generator = FrameGenerator::prepare(raw, key.size).ok()?;
            let source = format!(
                "{}\n{}",
                include_str!("../../ui_composite.wgsl"),
                include_str!("composition.wgsl")
            );
            let module = raw.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("frame generation composition"),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
            let pipeline = |entry, format| {
                raw.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(entry),
                    layout: None,
                    vertex: wgpu::VertexState {
                        module: &module,
                        entry_point: Some("composite_vertex"),
                        compilation_options: Default::default(),
                        buffers: &[],
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &module,
                        entry_point: Some(entry),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format,
                            blend: None,
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    primitive: Default::default(),
                    depth_stencil: None,
                    multisample: Default::default(),
                    multiview_mask: None,
                    cache: None,
                })
            };
            let world = image(
                raw,
                key.size.display,
                COLOR_FORMAT,
                1,
                wgpu::TextureUsages::COPY_DST
                    | wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::TEXTURE_BINDING,
            );
            let real = image(
                raw,
                key.size.display,
                render_scene::SCENE_COLOR_FORMAT,
                1,
                wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC,
            );
            let real_view = real.create_view(&Default::default());
            let depth = ResolvedDepth::from_source(
                &device,
                depth_source,
                wgpu::Extent3d {
                    width: key.size.render[0],
                    height: key.size.render[1],
                    depth_or_array_layers: 1,
                },
                key.samples,
                render_scene::RuntimeStage::GpuPost,
            )?;
            Some(Self {
                overlay: pipeline(
                    if key.samples > 1 {
                        "overlay_multi"
                    } else {
                        "overlay_single"
                    },
                    render_scene::SCENE_COLOR_FORMAT,
                ),
                copy_generated: pipeline("copy_generated", render_scene::SCENE_COLOR_FORMAT),
                copy_native: pipeline("copy_linear", render_scene::SCENE_COLOR_FORMAT),
                present: pipeline("copy_linear", key.output),
                hand: Layer::new(raw, key.size.render, key.samples, true),
                labels: Layer::new(raw, key.size.render, key.samples, false),
                sampler: raw.create_sampler(&wgpu::SamplerDescriptor {
                    mag_filter: wgpu::FilterMode::Linear,
                    min_filter: wgpu::FilterMode::Linear,
                    ..Default::default()
                }),
                key,
                world,
                _real: real,
                real_view,
                depth,
                temporal: std::sync::Mutex::new(Temporal {
                    generator,
                    history: None,
                    current: None,
                    world_captured: false,
                    real_captured: false,
                    reset: true,
                }),
            })
        })();
        let allocation_error = bevy::tasks::block_on(allocation.pop());
        let validation_error = bevy::tasks::block_on(validation.pop());
        let failed = allocation_error.or(validation_error);
        // Always pop the validation scope, including after an allocation failure.
        if failed.is_some() { None } else { result }
    }

    /// Resets per-frame observations without retaining a stale UI or world snapshot; returns
    /// whether the camera continues the retained history.
    pub fn begin_frame(&self, current: crate::motion_blur::history::CameraHistory) -> bool {
        let mut state = self.temporal.lock().expect("frame generation history");
        state.world_captured = false;
        state.real_captured = false;
        state.reset = state.history.is_none_or(|old| !old.same_anchor(current));
        state.current = Some(current);
        if state.reset {
            state.generator.invalidate();
        }
        !state.reset
    }

    /// Encodes a prepared full-coverage pipeline into the destination without sampling it.
    pub fn blit(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        pipeline: &wgpu::RenderPipeline,
        source: &wgpu::TextureView,
        destination: &wgpu::TextureView,
    ) {
        let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("frame generation copy"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(source),
            }],
        });
        encode(encoder, pipeline, &binding, destination);
    }

    /// Composes sharp names then the current hand over a native real or generated world.
    pub fn compose(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::TextureView,
        destination: &wgpu::TextureView,
    ) {
        let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("frame generation foreground"),
            layout: &self.overlay.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(self.hand.sampled()),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(source),
                },
                wgpu::BindGroupEntry {
                    binding: if self.key.samples > 1 { 2 } else { 3 },
                    resource: wgpu::BindingResource::TextureView(&self.labels.view),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        encode(encoder, &self.overlay, &binding, destination);
    }
}

/// Writes a complete attachment; all pipelines in this owner use the same fullscreen vertex.
pub(super) fn encode(
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::RenderPipeline,
    binding: &wgpu::BindGroup,
    destination: &wgpu::TextureView,
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("frame generation composition"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: destination,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                store: wgpu::StoreOp::Store,
            },
        })],
        ..Default::default()
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, binding, &[]);
    pass.draw(0..3, 0..1);
}
