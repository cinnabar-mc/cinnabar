//! Barrier, light-block and structure-void markers, drawn only while their layer is shown.
//!
//! Meshing records marker cells per sub-chunk; queue application keeps them here beside the
//! chunk entities. While [`TerrainMarkerLayers`] is empty nothing is extracted, uploaded or
//! queued. A shown layer rebuilds its instances only when the cells, layers or texture assets
//! change, resolving each block's art from the current pack. Instances live in fixed-size
//! vertex buffers, so no device binding limit bounds how many markers draw, and flipbook art
//! follows the shared terrain animation clock on the GPU.

use assets::{
    ANIMATION_FLAG_BLEND, Animation, BlockFace, DIAGNOSTIC_MATERIAL, Material, NetworkIdMode,
    RuntimeAssets, TerrainMarker, TextureRef,
};
use bevy::mesh::VertexBufferLayout;
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
            BufferDescriptor, BufferId, BufferInitDescriptor, BufferSize, BufferUsages, Canonical,
            ColorTargetState, ColorWrites, CompareFunction, DepthStencilState, FilterMode,
            FragmentState, PipelineCache, PrimitiveState, RenderPipeline, RenderPipelineDescriptor,
            Sampler, SamplerBindingType, SamplerDescriptor, ShaderStages, ShaderType, Specializer,
            SpecializerKey, TextureSampleType, TextureViewDimension, Variants, VertexAttribute,
            VertexFormat, VertexState, VertexStepMode,
        },
        renderer::{RenderDevice, RenderQueue},
        sync_world::MainEntity,
        view::{ExtractedView, ViewUniform, ViewUniformOffset, ViewUniforms},
    },
};
use meshing::PackedTerrainMarker;
use render_scene::TerrainMarkerLayers;
use render_scene::chunk::chunk_origin;
use render_scene::chunk::textures::{
    ChunkAnimationClock, ChunkTextureAssetIdentity, ChunkTextureAssets, ChunkTextureBindingIdentity,
};
use std::{collections::HashMap, ops::Range, sync::Arc};
use world::SubChunkKey;

use super::gpu::bind_groups::{ChunkGpuAnimationClock, ChunkGpuTextureAssets};

const MARKER_SHADER_HANDLE: Handle<Shader> = uuid_handle!("3f6c2a5e-8b1d-4c7a-9e24-5d0b7a1c6e93");
const INSTANCE_BYTES: u64 = size_of::<MarkerInstance>() as u64;
/// Instances per vertex buffer: 2.5 MiB, far below every device's buffer size limit.
const CHUNK_INSTANCES: usize = 1 << 16;
const SPRITE_VERTICES: u32 = 6;
const BOX_VERTICES: u32 = 36;
const BOX_FLAG: u32 = 1;
const LIGHT_SHIFT: u32 = 8;

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

/// One marker as the vertex stage reads it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
struct MarkerInstance {
    block: [i32; 3],
    /// Box flag and cell light; see the shader's `marker_vertex`.
    packed: u32,
    /// Each face's art index in [`MarkerTables::animations`], in [`BlockFace::ALL`] order for
    /// a box; a sprite uses the first. Keeping the per-cell choice here lets the bound tables
    /// grow only with the pack's distinct art.
    art: [u32; 6],
}

/// One frame of marker art: its texture page, array layer, exposed UV scale and the source
/// texture's size relative to its page, which keeps mip selection at the source resolution.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
struct MarkerTexture {
    page: u32,
    layer: u32,
    uv_scale: f32,
    reserved: u32,
    gradient_scale: [f32; 2],
}

/// The frames of one marker's art; static art is a single frame and missing art none.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
struct MarkerAnimation {
    frame_start: u32,
    frame_count: u32,
    ticks_per_frame: u32,
    flags: u32,
}

/// What one face's art is drawn from. Equal sources share one table entry and its frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ArtSource {
    Missing,
    Still(TextureRef),
    Flipbook(u32),
}

/// The art of every shown marker, rebuilt with the instances.
///
/// Entries are shared by source, so both tables are bounded by the pack's own textures and
/// flipbooks however many cells, alternatives or face combinations are shown.
#[derive(Default, Debug)]
struct MarkerTables {
    animations: Vec<MarkerAnimation>,
    textures: Vec<MarkerTexture>,
    indices: HashMap<ArtSource, u32>,
}

impl MarkerTables {
    fn clear(&mut self) {
        self.animations.clear();
        self.textures.clear();
        self.indices.clear();
    }

    /// The index of `material`'s art: its flipbook frames, or its one texture, appended the
    /// first time that source is seen. A missing material has art with no frames, which
    /// draws nothing.
    fn push(
        &mut self,
        material: Option<&Material>,
        animations: &[Animation],
        frames: &[TextureRef],
        texture: impl Fn(TextureRef) -> MarkerTexture,
    ) -> u32 {
        let flipbook = material.and_then(|material| {
            let animation = animations
                .get(material.animation as usize)
                .filter(|animation| animation.ticks_per_frame != 0)?;
            let start = animation.frame_start as usize;
            let frames = frames.get(start..start.checked_add(animation.frame_count as usize)?)?;
            (!frames.is_empty()).then_some((material.animation, animation, frames))
        });
        let source = match (material, flipbook) {
            (None, _) => ArtSource::Missing,
            (Some(_), Some((index, ..))) => ArtSource::Flipbook(index),
            (Some(material), None) => ArtSource::Still(material.texture),
        };
        if let Some(&index) = self.indices.get(&source) {
            return index;
        }
        let frame_start = self.textures.len() as u32;
        let (frame_count, ticks_per_frame, flags) = match (material, flipbook) {
            (None, _) => (0, 1, 0),
            (Some(_), Some((_, animation, frames))) => {
                self.textures
                    .extend(frames.iter().map(|&frame| texture(frame)));
                (
                    frames.len() as u32,
                    animation.ticks_per_frame,
                    animation.flags & ANIMATION_FLAG_BLEND,
                )
            }
            (Some(material), None) => {
                self.textures.push(texture(material.texture));
                (1, 1, 0)
            }
        };
        let index = self.animations.len() as u32;
        self.animations.push(MarkerAnimation {
            frame_start,
            frame_count,
            ticks_per_frame,
            flags,
        });
        self.indices.insert(source, index);
        index
    }
}

/// Instances of the shown layers, rebuilt only when their inputs change.
#[derive(Resource, Default)]
struct MarkerBatch {
    inputs: Option<(u64, u8, ChunkTextureAssetIdentity)>,
    /// Sprites first, then boxes.
    instances: Vec<MarkerInstance>,
    sprites: usize,
    tables: MarkerTables,
    uploaded: bool,
}

impl MarkerBatch {
    fn clear(&mut self) {
        self.inputs = None;
        self.instances.clear();
        self.sprites = 0;
        self.tables.clear();
        self.uploaded = false;
    }
}

/// Collects the instances of every cell whose layer is shown and whose block `art` resolves.
fn marker_instances(
    sections: &TerrainMarkerSections,
    layers: u8,
    mut art: impl FnMut(u32, [i32; 3]) -> Option<[u32; 6]>,
    sprites: &mut Vec<MarkerInstance>,
    boxes: &mut Vec<MarkerInstance>,
) {
    sprites.clear();
    boxes.clear();
    for (&key, markers) in &sections.sections {
        let origin = chunk_origin(key);
        for marker in markers.iter() {
            let class = marker.marker();
            if layers & class.layer() == 0 {
                continue;
            }
            let local = marker.local();
            let block = std::array::from_fn(|axis| origin[axis] + i32::from(local[axis]));
            let Some(art) = art(marker.block(), block) else {
                continue;
            };
            let boxed = !class.is_camera_facing();
            let instance = MarkerInstance {
                block,
                packed: if boxed { BOX_FLAG } else { 0 }
                    | (u32::from(marker.light()) << LIGHT_SHIFT),
                art,
            };
            if boxed {
                boxes.push(instance);
            } else {
                sprites.push(instance);
            }
        }
    }
}

/// The base materials a marker block draws with per art face, in [`BlockFace::ALL`] order: the
/// up face for a sprite, all six for a box. A face without art holds [`DIAGNOSTIC_MATERIAL`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct MarkerFaces {
    materials: [u32; 6],
    count: usize,
}

impl MarkerFaces {
    /// The faces of a marker block in `assets`, if that pack gives it any art.
    fn of(assets: &RuntimeAssets, block: u32) -> Option<Self> {
        let resolved = assets.resolve_known(NetworkIdMode::Sequential, block)?;
        let marker = TerrainMarker::from_variant(resolved.kind(), resolved.variant())?;
        let faces = if marker.is_camera_facing() {
            &[BlockFace::Up][..]
        } else {
            &BlockFace::ALL[..]
        };
        let mut materials = [DIAGNOSTIC_MATERIAL; 6];
        for (slot, &face) in materials.iter_mut().zip(faces) {
            *slot = resolved.face(face).material_id();
        }
        let faces = Self {
            materials,
            count: faces.len(),
        };
        faces
            .drawn()
            .iter()
            .any(|&material| material != DIAGNOSTIC_MATERIAL)
            .then_some(faces)
    }

    fn drawn(&self) -> &[u32] {
        &self.materials[..self.count]
    }

    /// The weighted texture alternative each face draws at the world cell `position`, chosen
    /// as terrain chooses it.
    fn at(mut self, materials: &[Material], position: [i32; 3]) -> Self {
        for material in &mut self.materials[..self.count] {
            if *material != DIAGNOSTIC_MATERIAL {
                *material = assets::positional_material_id(materials, *material, position);
            }
        }
        self
    }

    /// Each face's art index in `tables`, adding art the tables do not hold yet.
    fn push(self, assets: &RuntimeAssets, tables: &mut MarkerTables) -> [u32; 6] {
        let mut art = [0; 6];
        for (slot, &material) in art.iter_mut().zip(self.drawn()) {
            let material = (material != DIAGNOSTIC_MATERIAL)
                .then(|| assets.materials().get(material as usize))
                .flatten();
            *slot = tables.push(
                material,
                assets.animations(),
                assets.animation_frames(),
                |texture| marker_texture(assets, texture),
            );
        }
        art
    }
}

/// The page placement of `texture` and the scales that sample its exposed cell as terrain does.
///
/// Source sizes count the exposed cell's pixels. Terrain encodes the whole grid,
/// `size << grid`, and multiplies by the cell's `2^-grid` UV scale, so both select mips
/// by `size / page` per block.
fn marker_texture(assets: &RuntimeAssets, texture: TextureRef) -> MarkerTexture {
    let page = assets
        .texture_pages()
        .get(texture.page() as usize)
        .and_then(|page| page.texture.mips.first())
        .map_or(assets::TILE_SIZE, |mip| mip.size) as f32;
    MarkerTexture {
        page: texture.page(),
        layer: texture.layer(),
        uv_scale: (-f32::from(assets.texture_source_grid(texture))).exp2(),
        reserved: 0,
        gradient_scale: assets
            .texture_source_size(texture)
            .map(|size| size as f32 / page),
    }
}

/// Copies shown markers into the render world; hidden layers cost one comparison.
fn extract_markers(
    layers: Extract<Option<Res<TerrainMarkerLayers>>>,
    sections: Extract<Res<TerrainMarkerSections>>,
    textures: Extract<Res<ChunkTextureAssets>>,
    mut batch: ResMut<MarkerBatch>,
) {
    let layers = layers.as_deref().map_or(0, |layers| layers.0);
    if layers == 0 {
        if batch.inputs.is_some() {
            batch.clear();
        }
        return;
    }
    let inputs = Some((sections.revision, layers, textures.identity()));
    if batch.inputs == inputs {
        return;
    }
    let batch = &mut *batch;
    let assets = &*textures.assets;
    batch.tables.clear();
    let mut by_block = HashMap::new();
    let mut by_faces = HashMap::new();
    let mut boxes = Vec::new();
    marker_instances(
        &sections,
        layers,
        |block, position| {
            let faces = (*by_block
                .entry(block)
                .or_insert_with(|| MarkerFaces::of(assets, block)))?
            .at(assets.materials(), position);
            Some(
                *by_faces
                    .entry(faces)
                    .or_insert_with(|| faces.push(assets, &mut batch.tables)),
            )
        },
        &mut batch.instances,
        &mut boxes,
    );
    batch.sprites = batch.instances.len();
    batch.instances.append(&mut boxes);
    batch.inputs = inputs;
    batch.uploaded = false;
}

/// Each vertex buffer's sprite and box instance ranges, local to that buffer.
fn chunk_draws(total: usize, sprites: usize) -> impl Iterator<Item = (Range<u32>, Range<u32>)> {
    (0..total.div_ceil(CHUNK_INSTANCES)).map(move |chunk| {
        let start = chunk * CHUNK_INSTANCES;
        let end = (start + CHUNK_INSTANCES).min(total);
        let split = sprites.clamp(start, end);
        let local = |index: usize| (index - start) as u32;
        (local(start)..local(split), local(split)..local(end))
    })
}

type MarkerBindKey = (
    BufferId,
    BufferId,
    BufferId,
    BufferId,
    ChunkTextureBindingIdentity,
);

#[derive(Resource)]
struct MarkerGpu {
    sampler: Sampler,
    /// Instance buffers of [`CHUNK_INSTANCES`] each, in batch order.
    chunks: Vec<Buffer>,
    /// The batch's animation and texture tables.
    tables: Option<(Buffer, Buffer)>,
    bind_group: Option<BindGroup>,
    bound: Option<MarkerBindKey>,
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
        chunks: Vec::new(),
        tables: None,
        bind_group: None,
        bound: None,
    });
}

/// Writes a changed batch into its instance buffers and tables, and frees them once hidden.
fn prepare_markers(
    mut batch: ResMut<MarkerBatch>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    mut gpu: ResMut<MarkerGpu>,
) {
    if batch.instances.is_empty() {
        if !gpu.chunks.is_empty() || gpu.tables.is_some() {
            gpu.chunks.clear();
            gpu.tables = None;
            gpu.bind_group = None;
            gpu.bound = None;
        }
        return;
    }
    if batch.uploaded {
        return;
    }
    let needed = batch.instances.len().div_ceil(CHUNK_INSTANCES);
    gpu.chunks.truncate(needed);
    while gpu.chunks.len() < needed {
        let buffer = device.create_buffer(&BufferDescriptor {
            label: Some("terrain marker instances"),
            size: CHUNK_INSTANCES as u64 * INSTANCE_BYTES,
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        gpu.chunks.push(buffer);
    }
    for (buffer, instances) in gpu
        .chunks
        .iter()
        .zip(batch.instances.chunks(CHUNK_INSTANCES))
    {
        queue.write_buffer(buffer, 0, bytemuck::cast_slice(instances));
    }
    let table = |label, contents: &[u8]| {
        device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some(label),
            contents,
            usage: BufferUsages::STORAGE,
        })
    };
    gpu.tables = Some((
        table(
            "terrain marker animations",
            bytemuck::cast_slice(&batch.tables.animations),
        ),
        table(
            "terrain marker textures",
            bytemuck::cast_slice(&batch.tables.textures),
        ),
    ));
    batch.uploaded = true;
}

/// Binds the uploaded batch once the GPU texture pages match the assets it was built from.
#[allow(clippy::too_many_arguments, reason = "Bevy system parameters")]
fn prepare_marker_bind_group(
    batch: Res<MarkerBatch>,
    device: Res<RenderDevice>,
    pipeline_cache: Res<PipelineCache>,
    pipeline: Res<MarkerPipeline>,
    view_uniforms: Res<ViewUniforms>,
    textures: Res<ChunkGpuTextureAssets>,
    clock: Option<Res<ChunkGpuAnimationClock>>,
    mut gpu: ResMut<MarkerGpu>,
) {
    if batch.instances.is_empty() {
        return;
    }
    let (Some(view_binding), Some(view_buffer), Some(prepared), Some(clock), Some(tables)) = (
        view_uniforms.uniforms.binding(),
        view_uniforms.uniforms.buffer(),
        textures.prepared.as_ref(),
        clock.as_deref(),
        gpu.tables.as_ref(),
    ) else {
        gpu.bind_group = None;
        return;
    };
    // Pages still being prepared for a reloaded pack would show the wrong art.
    if batch.inputs.map(|(_, _, identity)| identity) != Some(prepared.identity.carrier) {
        gpu.bind_group = None;
        return;
    }
    let key = (
        view_buffer.id(),
        tables.0.id(),
        tables.1.id(),
        clock.buffer.id(),
        prepared.identity,
    );
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
            resource: tables.0.as_entire_binding(),
        },
        BindGroupEntry {
            binding: 2,
            resource: tables.1.as_entire_binding(),
        },
        BindGroupEntry {
            binding: 3,
            resource: clock.buffer.as_entire_binding(),
        },
    ];
    entries.extend(
        prepared
            .native_leaf_views
            .iter()
            .zip(PAGE_BINDINGS_START..)
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

/// The first texture page's binding, after the view, the two art tables and the clock.
const PAGE_BINDINGS_START: u32 = 4;
/// The sampler's binding, after one binding per texture page.
const PAGE_BINDINGS_END: u32 = PAGE_BINDINGS_START + assets::MAX_TEXTURE_PAGES as u32;

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
            table_entry(1, size_of::<MarkerAnimation>()),
            table_entry(2, size_of::<MarkerTexture>()),
            BindGroupLayoutEntry {
                binding: 3,
                visibility: ShaderStages::VERTEX,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: Some(ChunkAnimationClock::min_size()),
                },
                count: None,
            },
        ];
        entries.extend((PAGE_BINDINGS_START..PAGE_BINDINGS_END).map(|binding| {
            BindGroupLayoutEntry {
                binding,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            }
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
                buffers: vec![VertexBufferLayout {
                    array_stride: INSTANCE_BYTES,
                    step_mode: VertexStepMode::Instance,
                    attributes: vec![
                        VertexAttribute {
                            format: VertexFormat::Sint32x3,
                            offset: 0,
                            shader_location: 0,
                        },
                        VertexAttribute {
                            format: VertexFormat::Uint32,
                            offset: 12,
                            shader_location: 1,
                        },
                        VertexAttribute {
                            format: VertexFormat::Uint32x3,
                            offset: 16,
                            shader_location: 2,
                        },
                        VertexAttribute {
                            format: VertexFormat::Uint32x3,
                            offset: 28,
                            shader_location: 3,
                        },
                    ],
                }],
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

/// A read-only storage table the vertex stage indexes, holding `record` bytes per entry.
fn table_entry(binding: u32, record: usize) -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding,
        visibility: ShaderStages::VERTEX,
        ty: BindingType::Buffer {
            ty: BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: BufferSize::new(record as u64),
        },
        count: None,
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
    batch: Res<MarkerBatch>,
    pipeline_cache: Res<PipelineCache>,
    mut pipeline: ResMut<MarkerPipeline>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    draw_functions: Res<DrawFunctions<Transparent3d>>,
    views: Query<(Entity, &MainEntity, &ExtractedView, &ExtractedCamera, &Msaa)>,
) {
    // Preparation runs after queueing; a batch not yet uploaded or bound skips at draw time.
    if batch.instances.is_empty() {
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
    type Param = (SRes<MarkerBatch>, SRes<MarkerGpu>);
    type ViewQuery = ();
    type ItemQuery = ();

    fn render<'w>(
        _item: &P,
        _view: ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        (batch, gpu): SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let (batch, gpu) = (batch.into_inner(), gpu.into_inner());
        if !batch.uploaded {
            return RenderCommandResult::Skip;
        }
        let draws = chunk_draws(batch.instances.len(), batch.sprites);
        for (buffer, (sprites, boxes)) in gpu.chunks.iter().zip(draws) {
            pass.set_vertex_buffer(0, buffer.slice(..));
            if !sprites.is_empty() {
                pass.draw(0..SPRITE_VERTICES, sprites);
            }
            if !boxes.is_empty() {
                pass.draw(0..BOX_VERTICES, boxes);
            }
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
        .init_resource::<MarkerBatch>()
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
