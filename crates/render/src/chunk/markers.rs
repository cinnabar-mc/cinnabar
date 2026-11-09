//! Barrier, light-block and structure-void markers, drawn only while their layer is shown.
//!
//! Meshing records marker cells per sub-chunk; queue application keeps them here beside the
//! chunk entities. While [`TerrainMarkerLayers`] is empty nothing is extracted, uploaded or
//! queued. A shown layer rebuilds its instances only when the cells, layers or textures change.

use bevy::{
    asset::{Handle, load_internal_asset, uuid_handle},
    core_pipeline::core_3d::{CORE_3D_DEPTH_FORMAT, Transparent3d},
    ecs::{
        query::ROQueryItem,
        system::{
            SystemParamItem,
            lifetimeless::{Read, SRes},
        },
    },
    prelude::{
        App, BevyError, Commands, Entity, FromWorld, IntoScheduleConfigs, Msaa, Query, Res, ResMut,
        Resource, Result, Shader, World, default,
    },
    render::{
        Extract, ExtractSchedule, Render, RenderApp, RenderStartup, RenderSystems,
        camera::ExtractedCamera,
        render_phase::{
            AddRenderCommand, DrawFunctions, PhaseItem, PhaseItemExtraIndex, RenderCommand,
            RenderCommandResult, SetItemPipeline, TrackedRenderPass, ViewSortedRenderPhases,
        },
        render_resource::{
            AddressMode, BindGroup, BindGroupEntry, BindGroupLayoutDescriptor,
            BindGroupLayoutEntry, BindingResource, BindingType, Buffer, BufferBindingType,
            BufferDescriptor, BufferId, BufferSize, BufferUsages, Canonical, ColorTargetState,
            ColorWrites, CompareFunction, DepthStencilState, FilterMode, FragmentState,
            PipelineCache, PrimitiveState, RenderPipeline, RenderPipelineDescriptor, Sampler,
            SamplerBindingType, SamplerDescriptor, ShaderStages, ShaderType, Specializer,
            SpecializerKey, TextureSampleType, TextureViewDimension, Variants, VertexState,
        },
        renderer::{RenderDevice, RenderQueue},
        sync_world::MainEntity,
        view::{ExtractedView, ViewUniform, ViewUniformOffset, ViewUniforms},
    },
};
use meshing::PackedTerrainMarker;
use render_scene::TerrainMarkerLayers;
use render_scene::chunk::chunk_origin;
use render_scene::chunk::textures::{ChunkTextureAssets, ChunkTextureBindingIdentity};
use std::{collections::HashMap, sync::Arc};
use world::SubChunkKey;

use super::gpu::bind_groups::ChunkGpuTextureAssets;

const MARKER_SHADER_HANDLE: Handle<Shader> = uuid_handle!("3f6c2a5e-8b1d-4c7a-9e24-5d0b7a1c6e93");
const INSTANCE_BYTES: u64 = size_of::<MarkerInstance>() as u64;
const MIN_CAPACITY: usize = 64;
const SPRITE_VERTICES: u32 = 6;
const BOX_VERTICES: u32 = 36;
const BOX_FLAG: u32 = 1;
const LIGHT_SHIFT: u32 = 8;
const PAGE_SHIFT: u32 = 16;

/// Marker cells of every applied near sub-chunk, kept in step with the chunk entities.
#[derive(Resource, Default)]
pub(in crate::chunk) struct TerrainMarkerSections {
    sections: HashMap<SubChunkKey, Arc<[PackedTerrainMarker]>>,
    revision: u64,
}

impl TerrainMarkerSections {
    /// Replaces the cells of `key` with `markers`; an empty list forgets the section.
    pub(in crate::chunk) fn set(&mut self, key: SubChunkKey, markers: &Arc<[PackedTerrainMarker]>) {
        if markers.is_empty() {
            self.remove(key);
        } else if self
            .sections
            .insert(key, markers.clone())
            .is_none_or(|previous| previous != *markers)
        {
            self.revision += 1;
        }
    }

    /// Forgets the cells of a withdrawn or emptied section.
    pub(in crate::chunk) fn remove(&mut self, key: SubChunkKey) {
        if self.sections.remove(&key).is_some() {
            self.revision += 1;
        }
    }

    /// Forgets every section when the world session ends.
    pub(in crate::chunk) fn clear(&mut self) {
        if !self.sections.is_empty() {
            self.sections.clear();
            self.revision += 1;
        }
    }
}

/// One marker as the shader reads it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
struct MarkerInstance {
    block: [i32; 3],
    /// Box flag, cell light and texture page; see the shader's `Marker`.
    packed: u32,
    layer: u32,
    uv_scale: f32,
    reserved: [u32; 2],
}

/// Instances of the shown layers, rebuilt only when their inputs change.
#[derive(Resource, Default)]
struct MarkerFrame {
    inputs: Option<(u64, u8, u64)>,
    sprites: Vec<MarkerInstance>,
    boxes: Vec<MarkerInstance>,
    uploaded: bool,
}

impl MarkerFrame {
    fn is_empty(&self) -> bool {
        self.sprites.is_empty() && self.boxes.is_empty()
    }

    fn clear(&mut self) {
        self.inputs = None;
        self.sprites.clear();
        self.boxes.clear();
        self.uploaded = false;
    }
}

/// Collects the instances of every cell whose layer is shown.
fn marker_instances(
    sections: &TerrainMarkerSections,
    layers: u8,
    textures: &ChunkTextureAssets,
    sprites: &mut Vec<MarkerInstance>,
    boxes: &mut Vec<MarkerInstance>,
) {
    sprites.clear();
    boxes.clear();
    let assets = &textures.assets;
    for (&key, markers) in &sections.sections {
        let origin = chunk_origin(key);
        for marker in markers.iter() {
            let class = marker.marker();
            if layers & class.layer() == 0 {
                continue;
            }
            let Some(material) = assets.materials().get(marker.material() as usize) else {
                continue;
            };
            let texture = material.texture;
            let local = marker.local();
            let boxed = !class.is_camera_facing();
            let instance = MarkerInstance {
                block: std::array::from_fn(|axis| origin[axis] + i32::from(local[axis])),
                packed: if boxed { BOX_FLAG } else { 0 }
                    | (u32::from(marker.light()) << LIGHT_SHIFT)
                    | (texture.page() << PAGE_SHIFT),
                layer: texture.layer(),
                uv_scale: (-f32::from(assets.texture_source_grid(texture))).exp2(),
                reserved: [0; 2],
            };
            if boxed {
                boxes.push(instance);
            } else {
                sprites.push(instance);
            }
        }
    }
}

/// Copies shown markers into the render world; hidden layers cost one comparison.
fn extract_markers(
    layers: Extract<Option<Res<TerrainMarkerLayers>>>,
    sections: Extract<Res<TerrainMarkerSections>>,
    textures: Extract<Res<ChunkTextureAssets>>,
    mut frame: ResMut<MarkerFrame>,
) {
    let layers = layers.as_deref().map_or(0, |layers| layers.0);
    if layers == 0 {
        if frame.inputs.is_some() {
            frame.clear();
        }
        return;
    }
    let inputs = Some((sections.revision, layers, textures.revision));
    if frame.inputs == inputs {
        return;
    }
    let frame = &mut *frame;
    marker_instances(
        &sections,
        layers,
        &textures,
        &mut frame.sprites,
        &mut frame.boxes,
    );
    frame.inputs = inputs;
    frame.uploaded = false;
}

#[derive(Resource)]
struct MarkerGpu {
    sampler: Sampler,
    buffer: Option<Buffer>,
    capacity: usize,
    bind_group: Option<BindGroup>,
    bound: Option<(BufferId, BufferId, ChunkTextureBindingIdentity)>,
}

fn init_marker_gpu(mut commands: Commands, device: Res<RenderDevice>) {
    let sampler = device.create_sampler(&SamplerDescriptor {
        label: Some("terrain marker sampler"),
        address_mode_u: AddressMode::ClampToEdge,
        address_mode_v: AddressMode::ClampToEdge,
        address_mode_w: AddressMode::ClampToEdge,
        mag_filter: FilterMode::Nearest,
        min_filter: FilterMode::Nearest,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..default()
    });
    commands.insert_resource(MarkerGpu {
        sampler,
        buffer: None,
        capacity: 0,
        bind_group: None,
        bound: None,
    });
}

/// Writes changed instances, growing the buffer to the next power of two when needed.
fn prepare_markers(
    mut frame: ResMut<MarkerFrame>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    mut gpu: ResMut<MarkerGpu>,
) {
    if frame.uploaded || frame.is_empty() {
        return;
    }
    let total = frame.sprites.len() + frame.boxes.len();
    if gpu.buffer.is_none() || gpu.capacity < total {
        let capacity = total.next_power_of_two().max(MIN_CAPACITY);
        gpu.buffer = Some(device.create_buffer(&BufferDescriptor {
            label: Some("terrain marker instances"),
            size: capacity as u64 * INSTANCE_BYTES,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
        gpu.capacity = capacity;
        gpu.bind_group = None;
    }
    let buffer = gpu.buffer.as_ref().expect("marker buffer was just ensured");
    queue.write_buffer(buffer, 0, bytemuck::cast_slice(&frame.sprites));
    queue.write_buffer(
        buffer,
        frame.sprites.len() as u64 * INSTANCE_BYTES,
        bytemuck::cast_slice(&frame.boxes),
    );
    frame.uploaded = true;
}

fn prepare_marker_bind_group(
    frame: Res<MarkerFrame>,
    device: Res<RenderDevice>,
    pipeline_cache: Res<PipelineCache>,
    pipeline: Res<MarkerPipeline>,
    view_uniforms: Res<ViewUniforms>,
    textures: Res<ChunkGpuTextureAssets>,
    mut gpu: ResMut<MarkerGpu>,
) {
    if frame.is_empty() {
        return;
    }
    let (Some(view_binding), Some(view_buffer), Some(prepared), Some(buffer)) = (
        view_uniforms.uniforms.binding(),
        view_uniforms.uniforms.buffer(),
        textures.prepared.as_ref(),
        gpu.buffer.as_ref(),
    ) else {
        gpu.bind_group = None;
        return;
    };
    let key = (view_buffer.id(), buffer.id(), prepared.identity);
    if gpu.bind_group.is_some() && gpu.bound == Some(key) {
        return;
    }
    let mut entries = vec![
        BindGroupEntry {
            binding: 0,
            resource: view_binding,
        },
        BindGroupEntry {
            binding: 1,
            resource: buffer.as_entire_binding(),
        },
    ];
    entries.extend(
        prepared
            .native_leaf_views
            .iter()
            .zip(2..)
            .map(|(view, binding)| BindGroupEntry {
                binding,
                resource: BindingResource::TextureView(view),
            }),
    );
    entries.push(BindGroupEntry {
        binding: PAGE_BINDINGS_END,
        resource: BindingResource::Sampler(&gpu.sampler),
    });
    let bind_group = device.create_bind_group(
        "terrain marker bind group",
        &pipeline_cache.get_bind_group_layout(&pipeline.bind_group_layout),
        &entries,
    );
    gpu.bind_group = Some(bind_group);
    gpu.bound = Some(key);
}

/// The sampler's binding, after the view, the instances and one binding per texture page.
const PAGE_BINDINGS_END: u32 = 2 + assets::MAX_TEXTURE_PAGES as u32;

struct MarkerSpecializer;

#[derive(Resource)]
struct MarkerPipeline {
    variants: Variants<RenderPipeline, MarkerSpecializer>,
    bind_group_layout: BindGroupLayoutDescriptor,
}

impl FromWorld for MarkerPipeline {
    fn from_world(_world: &mut World) -> Self {
        let mut entries = vec![
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::VERTEX | ShaderStages::FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: Some(ViewUniform::min_size()),
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::VERTEX,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(INSTANCE_BYTES),
                },
                count: None,
            },
        ];
        entries.extend((2..PAGE_BINDINGS_END).map(|binding| BindGroupLayoutEntry {
            binding,
            visibility: ShaderStages::FRAGMENT,
            ty: BindingType::Texture {
                sample_type: TextureSampleType::Float { filterable: true },
                view_dimension: TextureViewDimension::D2Array,
                multisampled: false,
            },
            count: None,
        }));
        entries.push(BindGroupLayoutEntry {
            binding: PAGE_BINDINGS_END,
            visibility: ShaderStages::FRAGMENT,
            ty: BindingType::Sampler(SamplerBindingType::Filtering),
            count: None,
        });
        let bind_group_layout =
            BindGroupLayoutDescriptor::new("terrain marker bind group layout", &entries);
        let descriptor = RenderPipelineDescriptor {
            label: Some("terrain marker pipeline".into()),
            layout: vec![bind_group_layout.clone(), crate::lighting::layout()],
            vertex: VertexState {
                shader: MARKER_SHADER_HANDLE,
                entry_point: Some("marker_vertex".into()),
                buffers: Vec::new(),
                ..default()
            },
            fragment: Some(FragmentState {
                shader: MARKER_SHADER_HANDLE,
                entry_point: Some("marker_fragment".into()),
                targets: vec![Some(ColorTargetState {
                    format: render_scene::SCENE_COLOR_FORMAT,
                    blend: None,
                    write_mask: ColorWrites::COLOR,
                })],
                ..default()
            }),
            // Sprites face the camera and boxes are seen from inside as well.
            primitive: PrimitiveState {
                cull_mode: None,
                ..default()
            },
            depth_stencil: Some(DepthStencilState {
                format: CORE_3D_DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(CompareFunction::GreaterEqual),
                stencil: default(),
                bias: default(),
            }),
            ..default()
        };
        Self {
            variants: Variants::new(MarkerSpecializer, descriptor),
            bind_group_layout,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, SpecializerKey)]
struct MarkerPipelineKey {
    msaa: Msaa,
    hdr: bool,
}

impl Specializer<RenderPipeline> for MarkerSpecializer {
    type Key = MarkerPipelineKey;

    fn specialize(
        &self,
        key: Self::Key,
        descriptor: &mut RenderPipelineDescriptor,
    ) -> Result<Canonical<Self::Key>, BevyError> {
        descriptor.multisample.count = key.msaa.samples();
        let target = descriptor.fragment.as_mut().unwrap().targets[0]
            .as_mut()
            .unwrap();
        target.format = if key.hdr {
            render_scene::SCENE_HDR_FORMAT
        } else {
            render_scene::SCENE_COLOR_FORMAT
        };
        Ok(key)
    }
}

impl crate::pipeline_warmup::PrewarmPipelines for MarkerPipeline {
    fn prewarm(
        &mut self,
        cache: &PipelineCache,
        view: crate::pipeline_warmup::WarmView,
        ids: &mut crate::pipeline_warmup::WarmupIds,
    ) -> Result<(), BevyError> {
        ids.push(self.variants.specialize(
            cache,
            MarkerPipelineKey {
                msaa: view.msaa,
                hdr: view.hdr,
            },
        )?);
        Ok(())
    }
}

/// Queues one depth-writing draw per view, ahead of translucent terrain like opaque particles.
fn queue_markers(
    frame: Res<MarkerFrame>,
    gpu: Res<MarkerGpu>,
    pipeline_cache: Res<PipelineCache>,
    mut pipeline: ResMut<MarkerPipeline>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    draw_functions: Res<DrawFunctions<Transparent3d>>,
    views: Query<(Entity, &MainEntity, &ExtractedView, &ExtractedCamera, &Msaa)>,
) {
    if frame.is_empty() || gpu.bind_group.is_none() {
        return;
    }
    let draw_function = draw_functions.read().id::<DrawMarkers>();
    for (view_entity, main_entity, view, camera, msaa) in &views {
        let Some(phase) = phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        let Ok(pipeline) = pipeline.variants.specialize(
            &pipeline_cache,
            MarkerPipelineKey {
                msaa: *msaa,
                hdr: camera.hdr,
            },
        ) else {
            continue;
        };
        crate::transparent_phase::add(
            phase,
            Transparent3d {
                sorting_info: bevy::core_pipeline::core_3d::TransparentSortingInfo3d::AlwaysOnTop,
                entity: (view_entity, *main_entity),
                pipeline,
                draw_function,
                distance: f32::NEG_INFINITY,
                batch_range: 0..1,
                extra_index: PhaseItemExtraIndex::None,
                indexed: false,
            },
        );
    }
}

type DrawMarkers = (
    SetItemPipeline,
    SetMarkerBindGroup<0>,
    crate::lighting::SetWorldLightmap,
    DrawMarkerInstances,
);

struct SetMarkerBindGroup<const I: usize>;

impl<P: PhaseItem, const I: usize> RenderCommand<P> for SetMarkerBindGroup<I> {
    type Param = SRes<MarkerGpu>;
    type ViewQuery = Read<ViewUniformOffset>;
    type ItemQuery = ();

    fn render<'w>(
        _item: &P,
        view_offset: ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        gpu: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let Some(bind_group) = &gpu.into_inner().bind_group else {
            return RenderCommandResult::Skip;
        };
        pass.set_bind_group(I, bind_group, &[view_offset.offset]);
        RenderCommandResult::Success
    }
}

struct DrawMarkerInstances;

impl<P: PhaseItem> RenderCommand<P> for DrawMarkerInstances {
    type Param = SRes<MarkerFrame>;
    type ViewQuery = ();
    type ItemQuery = ();

    fn render<'w>(
        _item: &P,
        _view: ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        frame: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let frame = frame.into_inner();
        if !frame.uploaded {
            return RenderCommandResult::Skip;
        }
        let sprites = frame.sprites.len() as u32;
        let boxes = frame.boxes.len() as u32;
        if sprites != 0 {
            pass.draw(0..SPRITE_VERTICES, 0..sprites);
        }
        if boxes != 0 {
            pass.draw(0..BOX_VERTICES, sprites..sprites + boxes);
        }
        RenderCommandResult::Success
    }
}

/// Registers the marker table, its extraction and the marker pass.
pub(in crate::chunk) fn install(app: &mut App) {
    app.init_resource::<TerrainMarkerSections>()
        .init_resource::<TerrainMarkerLayers>();
    if app.get_sub_app(RenderApp).is_none() {
        return;
    }
    crate::lighting::install(app);
    load_internal_asset!(
        app,
        MARKER_SHADER_HANDLE,
        "../terrain_markers.wesl",
        crate::shader_safety::from_wesl
    );
    crate::pipeline_warmup::register::<MarkerPipeline>(app);
    let render_app = app.sub_app_mut(RenderApp);
    crate::transparent_phase::install(render_app);
    crate::world_passes::install(render_app);
    render_app
        .init_resource::<MarkerFrame>()
        .init_resource::<MarkerPipeline>()
        .add_render_command::<Transparent3d, DrawMarkers>()
        .add_systems(RenderStartup, init_marker_gpu)
        .add_systems(ExtractSchedule, extract_markers)
        .add_systems(
            Render,
            (
                prepare_markers.in_set(RenderSystems::PrepareResources),
                prepare_marker_bind_group.in_set(RenderSystems::PrepareBindGroups),
                queue_markers
                    .in_set(crate::world_passes::WorldPassSystems)
                    .in_set(RenderSystems::Queue),
            ),
        );
}

#[cfg(test)]
#[path = "markers/tests.rs"]
mod tests;
