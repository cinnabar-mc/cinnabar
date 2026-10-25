//! Native-resolution UI composition after the world has finished rendering: for a reduced world
//! view, and for every view presenting an HDR signal, whose UI must stay at paper white.

use super::upscale::{FsrConstants, UpscalePass, UpscalePipeline, UpscalePipelines};
use crate::gpu_allocation::{self, FailedRequests};
use crate::hdr_output::{
    HdrOutputRequest, HdrStage, HdrStageConstants, HdrStageKey, HdrStagePipeline, output_encoding,
};
use bevy::{
    core_pipeline::blit::{BlitPipeline, BlitPipelineKey},
    prelude::*,
    render::{
        render_resource::*,
        renderer::{RenderContext, RenderDevice},
        view::{PostProcessWrite, ViewTarget},
    },
};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

/// An output-sized scene pair used only by screen-space UI.
#[derive(Component)]
pub(crate) struct NativeUiTarget {
    device: wgpu::Device,
    textures: [Texture; 2],
    views: [TextureView; 2],
    current: AtomicUsize,
    ready: AtomicBool,
    available: AtomicBool,
    presented: AtomicBool,
    output_format: TextureFormat,
    sampler: Sampler,
    seed_pipelines: UpscalePipelines,
    seed_bindings: Mutex<Vec<(TextureViewId, u32, BindGroup)>>,
    presentation: Presentation,
}

/// How the completed native scene reaches the output.
enum Presentation {
    /// A plain copy into an SDR output.
    Blit {
        pipeline: CachedRenderPipelineId,
        bindings: [BindGroup; 2],
    },
    /// SDR highlights expand into the float scene before UI, then the frame is encoded.
    Hdr {
        expand: CachedRenderPipelineId,
        encode: CachedRenderPipelineId,
        /// Bound by source view and constants; retired views are released every seed.
        bindings: Mutex<Vec<(TextureViewId, [u32; 3], BindGroup)>>,
    },
}

/// The native scene format: float for HDR outputs, which carry highlights above paper white,
/// otherwise the world's own format.
fn native_scene_format(target: &ViewTarget) -> TextureFormat {
    if target
        .out_texture_view_format()
        .and_then(output_encoding)
        .is_some()
    {
        render_scene::SCENE_HDR_FORMAT
    } else {
        target.main_texture_format()
    }
}

impl NativeUiTarget {
    /// Allocates an independent scene pair, retaining the actual output's format for presentation.
    fn new(
        device: &RenderDevice,
        extent: [u32; 2],
        formats: [TextureFormat; 2],
        pipelines: (UpscalePipelines, NativePresentPipelines),
        blit: &BlitPipeline,
        cache: &PipelineCache,
    ) -> Option<Self> {
        let [scene_format, output_format] = formats;
        let bytes = u64::from(extent[0])
            * u64::from(extent[1])
            * u64::from(scene_format.block_copy_size(None)?);
        let allocate = || {
            gpu_allocation::checked(device, "native-resolution UI scene", bytes, || {
                device.create_texture(&TextureDescriptor {
                    label: Some("native-resolution UI scene"),
                    size: Extent3d {
                        width: extent[0],
                        height: extent[1],
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: TextureDimension::D2,
                    format: scene_format,
                    usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
            })
        };
        let textures = [allocate()?, allocate()?];
        let views = textures
            .each_ref()
            .map(|texture| texture.create_view(&default()));
        let presentation = match pipelines.1 {
            NativePresentPipelines::Blit(pipeline) => Presentation::Blit {
                pipeline,
                bindings: views
                    .each_ref()
                    .map(|view| blit.create_bind_group(device, view, cache)),
            },
            NativePresentPipelines::Hdr { expand, encode } => Presentation::Hdr {
                expand,
                encode,
                bindings: Mutex::new(Vec::with_capacity(4)),
            },
        };
        Some(Self {
            device: device.wgpu_device().clone(),
            textures,
            views,
            current: AtomicUsize::new(0),
            ready: AtomicBool::new(false),
            available: AtomicBool::new(false),
            presented: AtomicBool::new(false),
            output_format,
            sampler: device.create_sampler(&SamplerDescriptor {
                label: Some("bilinear world upscale"),
                mag_filter: FilterMode::Linear,
                min_filter: FilterMode::Linear,
                ..default()
            }),
            seed_pipelines: pipelines.0,
            seed_bindings: Mutex::new(Vec::with_capacity(3)),
            presentation,
        })
    }

    /// Keeps only this frame's native scene and presentation observations.
    fn begin_frame(&self, cache: &PipelineCache, sharpen: bool) {
        let presentable = match &self.presentation {
            Presentation::Blit { pipeline, .. } => cache.get_render_pipeline(*pipeline).is_some(),
            Presentation::Hdr { expand, encode, .. } => {
                cache.get_render_pipeline(*expand).is_some()
                    && cache.get_render_pipeline(*encode).is_some()
            }
        };
        self.available.store(
            self.seed_pipelines.ready_pass(cache, sharpen).is_some() && presentable,
            Ordering::Relaxed,
        );
        self.current.store(0, Ordering::Relaxed);
        self.ready.store(false, Ordering::Relaxed);
        self.presented.store(false, Ordering::Relaxed);
    }

    /// Whether this target expands and encodes for an HDR output.
    fn presents_hdr(&self) -> bool {
        matches!(self.presentation, Presentation::Hdr { .. })
    }

    /// The format screen UI draws into.
    fn scene_format(&self) -> TextureFormat {
        self.textures[0].format()
    }

    /// The pipelines presentation waits for, for readiness checks.
    #[cfg(test)]
    pub(super) fn present_pipelines(&self) -> Vec<CachedRenderPipelineId> {
        match &self.presentation {
            Presentation::Blit { pipeline, .. } => vec![*pipeline],
            Presentation::Hdr { expand, encode, .. } => vec![*expand, *encode],
        }
    }

    /// Keeps preparation and drawing on the same target while pipelines compile.
    fn is_available(&self) -> bool {
        self.available.load(Ordering::Relaxed)
    }

    /// The native output dimensions, independent of the world texture's resolution.
    pub(crate) fn extent(&self) -> [u32; 2] {
        [self.textures[0].width(), self.textures[0].height()]
    }

    /// Whether this frame's world has been copied into the native UI scene.
    pub(crate) fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Relaxed)
    }

    /// Whether the completed native UI scene has been encoded into the real output this frame.
    pub(crate) fn was_presented(&self) -> bool {
        self.presented.load(Ordering::Relaxed)
    }

    /// Records an externally encoded final UI composite only after its scene was seeded.
    pub(crate) fn mark_presented(&self) {
        if self.is_ready() {
            self.presented.store(true, Ordering::Relaxed);
        }
    }

    /// Refuses stale dimensions, output formats, or resources from a replaced render device.
    fn matches(&self, device: &RenderDevice, target: &ViewTarget, extent: [u32; 2]) -> bool {
        &self.device == device.wgpu_device()
            && self.scene_format() == native_scene_format(target)
            && self.extent() == extent
            && target.out_texture_view_format() == Some(self.output_format)
    }

    /// Upscales the completed world once, before any native HUD or menu batches are encoded.
    /// For an HDR output the world is copied into the float scene, and SDR-tonemapped worlds have
    /// their highlights expanded, in the same pass when the world is not reduced.
    pub(crate) fn seed(
        &self,
        world: &World,
        context: &mut RenderContext,
        target: &ViewTarget,
    ) -> bool {
        if !self.matches(context.render_device(), target, self.extent())
            || target
                .out_texture()
                .map(|output| crate::render_bounds::extent(output))
                != Some(self.extent())
        {
            return false;
        }
        if self.is_ready() {
            return true;
        }
        let Some(cache) = world.get_resource::<PipelineCache>() else {
            return false;
        };
        let source = target.main_texture_view();
        let sdr_world = target.main_texture_format() != render_scene::SCENE_HDR_FORMAT;
        if let Presentation::Hdr { expand, .. } = &self.presentation
            && crate::render_bounds::extent(source) == self.extent()
        {
            let (Some(constants), Some(pipeline)) = (
                hdr_constants(world, sdr_world),
                cache.get_render_pipeline(*expand),
            ) else {
                return false;
            };
            let Some(binding) =
                self.hdr_binding(world, context.render_device(), target, source, constants)
            else {
                return false;
            };
            encode_blit(
                world,
                context,
                &self.views[0],
                pipeline,
                &binding,
                "HDR world expansion",
            );
            self.current.store(0, Ordering::Relaxed);
            self.ready.store(true, Ordering::Relaxed);
            return true;
        }
        if !self.upscale_world(world, context, target, cache) {
            return false;
        }
        if let Presentation::Hdr { expand, .. } = &self.presentation
            && sdr_world
        {
            let (Some(constants), Some(pipeline)) = (
                hdr_constants(world, true),
                cache.get_render_pipeline(*expand),
            ) else {
                return false;
            };
            let Some(binding) = self.hdr_binding(
                world,
                context.render_device(),
                target,
                &self.views[0],
                constants,
            ) else {
                return false;
            };
            encode_blit(
                world,
                context,
                &self.views[1],
                pipeline,
                &binding,
                "HDR world expansion",
            );
            self.current.store(1, Ordering::Relaxed);
        }
        self.ready.store(true, Ordering::Relaxed);
        true
    }

    /// Reconstructs the reduced world into the first native scene with FSR1 or its fallback.
    fn upscale_world(
        &self,
        world: &World,
        context: &mut RenderContext,
        target: &ViewTarget,
        cache: &PipelineCache,
    ) -> bool {
        let Some(upscale) = world.get_resource::<UpscalePipeline>() else {
            return false;
        };
        let sharpness = world
            .get_resource::<crate::render_scale::RenderScaleSharpen>()
            .and_then(|filter| filter.0);
        let Some(pass) = self.seed_pipelines.ready_pass(cache, sharpness.is_some()) else {
            return false;
        };
        let sharpen = pass == UpscalePass::Easu && sharpness.is_some();
        let source = target.main_texture_view();
        let live = [
            source.id(),
            target.main_texture_other_view().id(),
            self.views[1].id(),
        ];
        let mut bindings = self
            .seed_bindings
            .lock()
            .expect("native UI seed bindings lock");
        bindings.retain(|(id, _, _)| live.contains(id));
        let mut binding = |source: &TextureView, input, sharpness: Option<f32>| {
            let strength = sharpness.map_or(0.0, |stops| (-stops).exp2()).to_bits();
            if let Some((_, _, binding)) = bindings
                .iter()
                .find(|(id, key, _)| *id == source.id() && *key == strength)
            {
                return binding.clone();
            }
            bindings.retain(|(id, _, _)| *id != source.id());
            let binding = upscale.binding(
                context.render_device(),
                cache,
                source,
                &self.sampler,
                FsrConstants::new(
                    input,
                    self.extent(),
                    sharpness,
                    target.main_texture_format() == render_scene::SCENE_HDR_FORMAT,
                ),
            );
            bindings.push((source.id(), strength, binding.clone()));
            binding
        };
        let source_binding = binding(
            source,
            crate::render_bounds::extent(target.main_texture_view()),
            None,
        );
        let rcas_binding = sharpen.then(|| binding(&self.views[1], self.extent(), sharpness));
        drop(bindings);
        self.current.store(0, Ordering::Relaxed);
        if let Some(rcas_binding) = rcas_binding {
            encode_blit(
                world,
                context,
                &self.views[1],
                cache.get_render_pipeline(self.seed_pipelines.easu).unwrap(),
                &source_binding,
                "FSR1 EASU",
            );
            encode_blit(
                world,
                context,
                &self.views[0],
                cache.get_render_pipeline(self.seed_pipelines.rcas).unwrap(),
                &rcas_binding,
                "FSR1 RCAS",
            );
        } else {
            encode_blit(
                world,
                context,
                &self.views[0],
                cache
                    .get_render_pipeline(match pass {
                        UpscalePass::Easu => self.seed_pipelines.easu,
                        _ => self.seed_pipelines.bilinear,
                    })
                    .unwrap(),
                &source_binding,
                if pass == UpscalePass::Easu {
                    "FSR1 EASU"
                } else {
                    "bilinear world fallback"
                },
            );
        }
        true
    }

    /// Binds `source` to an HDR stage, keeping only bindings of live scene views and of the
    /// current expansion and encoding constants.
    fn hdr_binding(
        &self,
        world: &World,
        device: &RenderDevice,
        target: &ViewTarget,
        source: &TextureView,
        constants: HdrStageConstants,
    ) -> Option<BindGroup> {
        let Presentation::Hdr { bindings, .. } = &self.presentation else {
            return None;
        };
        let (Some(cache), Some(stages), Some(request)) = (
            world.get_resource::<PipelineCache>(),
            world.get_resource::<HdrStagePipeline>(),
            world.get_resource::<HdrOutputRequest>(),
        ) else {
            return None;
        };
        let live = [
            target.main_texture_view().id(),
            target.main_texture_other_view().id(),
            self.views[0].id(),
            self.views[1].id(),
        ];
        let current = [true, false].map(|expand| HdrStageConstants::new(request.0, expand).key());
        let key = constants.key();
        let mut bindings = bindings.lock().expect("HDR stage bindings lock");
        bindings.retain(|(id, held, _)| live.contains(id) && current.contains(held));
        if let Some((_, _, binding)) = bindings
            .iter()
            .find(|(id, held, _)| *id == source.id() && *held == key)
        {
            return Some(binding.clone());
        }
        let binding = stages.binding(device, cache, source, constants);
        bindings.push((source.id(), key, binding.clone()));
        Some(binding)
    }

    /// Presents even when no final alpha layer exists, including empty and invert-only HUDs.
    pub(crate) fn present(
        &self,
        world: &World,
        context: &mut RenderContext,
        target: &ViewTarget,
    ) -> bool {
        if !self.is_ready()
            || !self.matches(context.render_device(), target, self.extent())
            || target
                .out_texture()
                .map(|output| crate::render_bounds::extent(output))
                != Some(self.extent())
        {
            return false;
        }
        let Some(cache) = world.get_resource::<PipelineCache>() else {
            return false;
        };
        let index = self.current.load(Ordering::Relaxed);
        let (pipeline, binding, label) = match &self.presentation {
            Presentation::Blit { pipeline, bindings } => (
                cache.get_render_pipeline(*pipeline),
                Some(bindings[index].clone()),
                "native UI output",
            ),
            Presentation::Hdr { encode, .. } => (
                cache.get_render_pipeline(*encode),
                hdr_constants(world, false).and_then(|constants| {
                    self.hdr_binding(
                        world,
                        context.render_device(),
                        target,
                        &self.views[index],
                        constants,
                    )
                }),
                "HDR native UI output",
            ),
        };
        let (Some(pipeline), Some(binding)) = (pipeline, binding) else {
            return false;
        };
        // Obtaining the attachment also records usage for Bevy's surface presentation gate.
        let Some(output) = target.out_texture_color_attachment(None) else {
            return false;
        };
        encode_blit(world, context, output.view, pipeline, &binding, label);
        self.mark_presented();
        true
    }

    /// Changes the live native scene only when the caller will write the entire destination.
    fn post_process_write(&self) -> PostProcessWrite<'_> {
        let source = self.current.fetch_xor(1, Ordering::Relaxed);
        let destination = source ^ 1;
        PostProcessWrite {
            source: &self.views[source],
            source_texture: &self.textures[source],
            destination: &self.views[destination],
            destination_texture: &self.textures[destination],
        }
    }
}

/// The player's current HDR constants; `expand` lifts SDR highlights toward the peak.
fn hdr_constants(world: &World, expand: bool) -> Option<HdrStageConstants> {
    world
        .get_resource::<HdrOutputRequest>()
        .map(|request| HdrStageConstants::new(request.0, expand))
}

/// Whether screen UI draws into a float scene: the native target when one is in use this frame,
/// otherwise the world's own main texture.
pub(crate) fn ui_scene_hdr(camera_hdr: bool, native: Option<&NativeUiTarget>) -> bool {
    native
        .filter(|native| native.is_available())
        .map_or(camera_hdr, |native| {
            native.scene_format() == render_scene::SCENE_HDR_FORMAT
        })
}

/// Whether screen UI composites over highlights above paper white this frame.
pub(crate) fn ui_extended_range(native: Option<&NativeUiTarget>) -> bool {
    native.is_some_and(|native| native.is_available() && native.presents_hdr())
}

/// Encodes a full-coverage blit without sampling the output attachment itself.
fn encode_blit(
    world: &World,
    context: &mut RenderContext,
    destination: &wgpu::TextureView,
    pipeline: &RenderPipeline,
    binding: &BindGroup,
    label: &'static str,
) {
    let attachments = [Some(RenderPassColorAttachment {
        view: destination,
        depth_slice: None,
        resolve_target: None,
        ops: Operations {
            load: LoadOp::Clear(default()),
            store: StoreOp::Store,
        },
    })];
    let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some(label),
        color_attachments: &attachments,
        depth_stencil_attachment: None,
        timestamp_writes: crate::gpu_timing::render_pass_timestamps(
            world,
            render_scene::RuntimeStage::GpuBlit,
        ),
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_render_pipeline(pipeline);
    pass.set_bind_group(0, binding, &[]);
    pass.draw(0..3, 0..1);
}

/// Includes device, extent and both formats so changed allocation requests retry immediately.
type NativeUiRequest = (wgpu::Device, [u32; 2], TextureFormat, TextureFormat);

/// How a new native target presents, chosen from its output format.
#[derive(Clone, Copy)]
enum NativePresentPipelines {
    Blit(CachedRenderPipelineId),
    Hdr {
        expand: CachedRenderPipelineId,
        encode: CachedRenderPipelineId,
    },
}

/// The output extent a view's screen UI composes at: a reduced view's marker, or the full
/// output of a view presenting HDR, whose UI must not share the world's tonemapped range.
fn native_extent(
    target: &ViewTarget,
    scaled: Option<&crate::render_scale::RenderScaleOutput>,
) -> Option<[u32; 2]> {
    scaled.map(|output| output.0.to_array()).or_else(|| {
        target
            .out_texture_view_format()
            .and_then(output_encoding)
            .and(target.out_texture())
            .map(|output| crate::render_bounds::extent(output))
    })
}

/// Allocates only for an admitted reduced or HDR view whose real output still matches it.
#[allow(clippy::too_many_arguments)] // View admission and the pipeline owners share one allocation boundary.
pub(crate) fn prepare_native_ui_targets(
    mut commands: Commands,
    device: Res<RenderDevice>,
    cache: Res<PipelineCache>,
    blit: Option<Res<BlitPipeline>>,
    mut pipelines: Option<ResMut<SpecializedRenderPipelines<BlitPipeline>>>,
    mut upscale: Option<ResMut<UpscalePipeline>>,
    mut hdr: Option<ResMut<HdrStagePipeline>>,
    mut failed: Local<FailedRequests<NativeUiRequest>>,
    sharpen: Option<Res<crate::render_scale::RenderScaleSharpen>>,
    views: Query<(
        Entity,
        &ViewTarget,
        Option<&crate::render_scale::RenderScaleOutput>,
        Option<&NativeUiTarget>,
    )>,
    removed: Query<Entity, (With<NativeUiTarget>, Without<ViewTarget>)>,
) {
    failed.retain(|entity| {
        views
            .get(entity)
            .is_ok_and(|(_, target, scaled, _)| native_extent(target, scaled).is_some())
    });
    for entity in &removed {
        commands.entity(entity).remove::<NativeUiTarget>();
    }
    for (entity, target, scaled, previous) in &views {
        let extent = native_extent(target, scaled);
        let admitted = extent.filter(|extent| {
            !extent.contains(&0)
                && extent
                    .iter()
                    .all(|&value| value <= device.limits().max_texture_dimension_2d)
                && target
                    .out_texture()
                    .map(|output| crate::render_bounds::extent(output))
                    == Some(*extent)
                && [
                    render_scene::SCENE_COLOR_FORMAT,
                    render_scene::SCENE_HDR_FORMAT,
                ]
                .contains(&target.main_texture_format())
        });
        let (Some(extent), Some(output_format)) = (admitted, target.out_texture_view_format())
        else {
            if previous.is_some() {
                commands.entity(entity).remove::<NativeUiTarget>();
            }
            continue;
        };
        if let Some(previous) =
            previous.filter(|previous| previous.matches(&device, target, extent))
        {
            previous.begin_frame(
                &cache,
                sharpen.as_ref().is_some_and(|filter| filter.0.is_some()),
            );
            continue;
        }
        // Standalone UI preparation does not need the core pipeline's blit resources.
        let (Some(blit), Some(pipelines), Some(upscale)) = (
            blit.as_deref(),
            pipelines.as_deref_mut(),
            upscale.as_deref_mut(),
        ) else {
            if previous.is_some() {
                commands.entity(entity).remove::<NativeUiTarget>();
            }
            continue;
        };
        let scene_format = native_scene_format(target);
        let present = match (output_encoding(output_format), hdr.as_deref_mut()) {
            (Some(encoding), Some(hdr)) => NativePresentPipelines::Hdr {
                expand: hdr.specialize(
                    &cache,
                    HdrStageKey {
                        stage: HdrStage::Expand,
                        format: scene_format,
                    },
                ),
                encode: hdr.specialize(
                    &cache,
                    HdrStageKey {
                        stage: HdrStage::Encode(encoding),
                        format: output_format,
                    },
                ),
            },
            (Some(_), None) => {
                if previous.is_some() {
                    commands.entity(entity).remove::<NativeUiTarget>();
                }
                continue;
            }
            (None, _) => NativePresentPipelines::Blit(pipelines.specialize(
                &cache,
                blit,
                BlitPipelineKey {
                    target_format: output_format,
                    blend_state: None,
                    samples: 1,
                    source_space: None,
                },
            )),
        };
        let seeds = upscale.stages(&cache, scene_format);
        let request = (
            device.wgpu_device().clone(),
            extent,
            scene_format,
            output_format,
        );
        if let Some(native) = failed.attempt(entity, request, || {
            NativeUiTarget::new(
                &device,
                extent,
                [scene_format, output_format],
                (seeds, present),
                blit,
                &cache,
            )
        }) {
            native.begin_frame(
                &cache,
                sharpen.as_ref().is_some_and(|filter| filter.0.is_some()),
            );
            commands.entity(entity).insert(native);
        } else if previous.is_some() {
            commands.entity(entity).remove::<NativeUiTarget>();
        }
    }
}

/// Selects native screen composition without changing the world view or its depth attachments.
#[derive(Clone, Copy)]
pub(crate) struct UiTarget<'a> {
    output: &'a ViewTarget,
    native: Option<&'a NativeUiTarget>,
}

impl<'a> UiTarget<'a> {
    /// Preparation selects the native extent before the world's pixels are seeded later.
    pub(crate) fn new(output: &'a ViewTarget, native: Option<&'a NativeUiTarget>) -> Self {
        Self {
            output,
            native: native.filter(|native| native.is_available()),
        }
    }

    /// Reports whether screen UI has its own output-sized scene.
    pub(crate) fn is_native(self) -> bool {
        self.native.is_some()
    }
    /// Returns the current scene storage used by UI composition.
    pub(crate) fn main_texture(self) -> &'a Texture {
        self.native.map_or_else(
            || self.output.main_texture(),
            |native| &native.textures[native.current.load(Ordering::Relaxed)],
        )
    }
    #[cfg(test)]
    /// Returns the other scene storage for ping-pong lifetime checks.
    pub(crate) fn main_texture_other(self) -> &'a Texture {
        self.native.map_or_else(
            || self.output.main_texture_other(),
            |native| &native.textures[native.current.load(Ordering::Relaxed) ^ 1],
        )
    }
    /// Returns the current scene view used by UI draws.
    pub(crate) fn main_texture_view(self) -> &'a TextureView {
        self.native.map_or_else(
            || self.output.main_texture_view(),
            |native| &native.views[native.current.load(Ordering::Relaxed)],
        )
    }
    /// Returns the scene view reserved for the next full-coverage composite.
    pub(crate) fn main_texture_other_view(self) -> &'a TextureView {
        self.native.map_or_else(
            || self.output.main_texture_other_view(),
            |native| &native.views[native.current.load(Ordering::Relaxed) ^ 1],
        )
    }
    /// Swaps scene views only for a caller that writes the whole destination.
    pub(crate) fn post_process_write(self) -> PostProcessWrite<'a> {
        self.native.map_or_else(
            || self.output.post_process_write(),
            NativeUiTarget::post_process_write,
        )
    }
    /// Borrows the native presentation attachment independently of scene size.
    pub(crate) fn out_texture(self) -> Option<&'a TextureView> {
        self.output.out_texture()
    }
    /// Acquires the final attachment and records its use for presentation.
    pub(crate) fn out_texture_color_attachment(
        self,
        clear: Option<LinearRgba>,
    ) -> Option<RenderPassColorAttachment<'a>> {
        self.output.out_texture_color_attachment(clear)
    }
}

#[cfg(test)]
#[path = "target_tests.rs"]
mod tests;
