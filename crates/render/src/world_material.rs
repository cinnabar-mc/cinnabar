//! Programmable world materials shared by the actor and terrain renderers: each published
//! material's GPU resources (frame uniform, declared textures and sampler in one bind group)
//! and the composed shader modules both renderers draw it with.

use bevy::asset::uuid::Uuid;
use bevy::prelude::{Handle, Local, Res, ResMut, Resource, Shader};
use bevy::render::{
    render_resource::{
        AddressMode, BindGroup, BindGroupEntry, BindGroupLayoutDescriptor, BindGroupLayoutEntry,
        BindingResource, BindingType, Buffer, BufferBindingType, BufferDescriptor, BufferSize,
        BufferUsages, Extent3d, FilterMode, PipelineCache, SamplerBindingType, ShaderStages,
        TextureDataOrder, TextureDescriptor, TextureDimension, TextureFormat, TextureSampleType,
        TextureUsages, TextureViewDescriptor, TextureViewDimension,
    },
    renderer::{RenderDevice, RenderQueue},
};
use mod_render::world_material::{
    self as abi,
    publication::{Blend, Depth, Published},
};
use render_scene::world_material::WorldMaterialScene;
use std::collections::{BTreeSet, HashMap};

/// Shader namespaces: one composed module per material and renderer.
// The publication's slot limits are the tag widths meshing and the actor material word carry.
const _: () =
    assert!(abi::publication::MAX_BLOCK_SLOTS == meshing::PackedQuad::MAX_WORLD_MATERIAL as usize);
const _: () = assert!(
    abi::publication::MAX_ENTITY_SLOTS
        == (render_model::actor::frame::ACTOR_WORLD_MATERIAL_MASK
            >> render_model::actor::frame::ACTOR_WORLD_MATERIAL_SHIFT) as usize
);

pub(crate) const ACTOR_SHADERS: u64 = u64::from_le_bytes(*b"wmactor\0");
pub(crate) const TERRAIN_SHADERS: u64 = u64::from_le_bytes(*b"wmterra\0");
pub(crate) const MODEL_SHADERS: u64 = u64::from_le_bytes(*b"wmmodel\0");

/// What distinguishes a world material's pipelines from the base ones. Pipelines follow the
/// program, not the material, so materials and sessions declaring the same program share them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct WorldVariant {
    /// The program's content identity ([`abi::WorldMaterial::program`]).
    pub program: u128,
    pub textures: u8,
    pub blend: Blend,
    pub depth: Depth,
    /// The guest functions and resources its module has beyond `shade`.
    pub features: abi::Features,
    /// Whether its targets draw again in the effects pass, for a bloom or an outline.
    pub effects_pass: bool,
}

impl WorldVariant {
    pub(crate) fn of(published: &Published) -> Option<Self> {
        Some(Self {
            program: published.material.program(),
            textures: u8::try_from(published.textures.len()).ok()?,
            blend: published.blend,
            depth: published.depth,
            features: published.material.features,
            effects_pass: published.effects.bloom.is_some()
                || published.effects.outline_pixels.is_some(),
        })
    }

    /// Its bind group layout: the frame uniform, the textures and sampler, then its cells.
    pub(crate) fn layout(self) -> BindGroupLayoutDescriptor {
        layout(self.textures, self.features)
    }

    /// This program's module in the renderer `namespace`.
    pub(crate) fn shader(self, namespace: u64) -> Handle<Shader> {
        shader(namespace, self.program)
    }

    /// The shader definitions selecting the host's handling of the material's blend class and
    /// of the effect functions its module has.
    pub(crate) fn defs(self) -> [bevy::shader::ShaderDefVal; 6] {
        let def = |name: &'static str, enabled: bool| {
            bevy::shader::ShaderDefVal::Bool(name.into(), enabled)
        };
        [
            def("WORLD_MATERIAL", true),
            def("WORLD_MATERIAL_CUTOUT", self.blend == Blend::Cutout),
            def("WORLD_MATERIAL_OPAQUE", self.blend == Blend::Opaque),
            def(
                "WORLD_MATERIAL_PREMULTIPLY",
                matches!(self.blend, Blend::Premultiplied | Blend::Additive),
            ),
            def("WORLD_MATERIAL_DISPLACE", self.features.displace),
            def("WORLD_MATERIAL_SCENE", self.features.scene),
        ]
    }
}

/// The module of `program` in renderer `namespace`.
fn shader(namespace: u64, program: u128) -> Handle<Shader> {
    Handle::Uuid(
        Uuid::from_u64_pair(namespace ^ (program >> 64) as u64, program as u64),
        std::marker::PhantomData,
    )
}

/// Bind group layout of a material with `textures` declared textures and `features`: the frame
/// uniform, the textures, a sampler when there is one, then the compute cells it reads.
pub(crate) fn layout(textures: u8, features: abi::Features) -> BindGroupLayoutDescriptor {
    let mut entries = vec![BindGroupLayoutEntry {
        binding: 0,
        // Terrain vertex stages read the quad tag.
        visibility: ShaderStages::VERTEX_FRAGMENT,
        ty: BindingType::Buffer {
            ty: BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: BufferSize::new(abi::FRAME_UNIFORM_BYTES as u64),
        },
        count: None,
    }];
    for index in 0..u32::from(textures) {
        entries.push(BindGroupLayoutEntry {
            binding: index + 1,
            visibility: ShaderStages::FRAGMENT,
            ty: BindingType::Texture {
                sample_type: TextureSampleType::Float { filterable: true },
                view_dimension: TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        });
    }
    if textures != 0 {
        entries.push(BindGroupLayoutEntry {
            binding: u32::from(textures) + 1,
            visibility: ShaderStages::FRAGMENT,
            ty: BindingType::Sampler(SamplerBindingType::Filtering),
            count: None,
        });
    }
    if let Some(binding) = abi::MaterialBindings::of(usize::from(textures), features).cells {
        entries.push(BindGroupLayoutEntry {
            binding,
            visibility: ShaderStages::VERTEX_FRAGMENT,
            ty: BindingType::Buffer {
                ty: BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: BufferSize::new(16),
            },
            count: None,
        });
    }
    BindGroupLayoutDescriptor::new("world material bind group layout", &entries)
}

/// The session's animation phase, published from the main world each frame.
#[derive(
    Resource, bevy::render::extract_resource::ExtractResource, Clone, Copy, Default, PartialEq,
)]
#[extract_app(bevy::render::RenderApp)]
pub(crate) struct WorldMaterialPhase(pub f32);

/// Advances the phase from the wall clock: Unix time modulo the animation wrap, so a server can
/// start world material effects at a moment on the same clock and every client agrees.
pub(crate) fn advance_phase(phase: ResMut<WorldMaterialPhase>) {
    if let Ok(since_epoch) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        set_phase(phase, since_epoch);
    }
}

/// Sets the phase for `since_epoch` past the Unix epoch, touching the resource only when it
/// changes so an unchanged phase extracts nothing.
fn set_phase(mut phase: ResMut<WorldMaterialPhase>, since_epoch: std::time::Duration) {
    let next = mod_render::material::phase(since_epoch.as_secs_f64());
    if phase.0 != next {
        phase.0 = next;
    }
}

/// How one renderer composes a material's module: its namespace, the bind group the library
/// takes, and the module builder over the composed source and its path.
pub(crate) struct ShaderFamily {
    pub namespace: u64,
    pub group: u32,
    pub build: fn(String, String) -> Shader,
}

/// Registers each published program's composed modules for every renderer, once per program.
/// Modules stay registered after their materials are revoked: Bevy's pipeline cache cannot
/// drop a pipeline, and removing its module requeues it to retry the missing module every
/// frame, so a kept module leaves its compiled pipelines idle instead, and a later material or
/// session declaring the same program reuses them without compiling.
pub(crate) fn register_shaders(
    families: &[ShaderFamily],
    scene: &WorldMaterialScene,
    shaders: &mut bevy::asset::Assets<Shader>,
    registered: &mut BTreeSet<u128>,
) {
    register_computes(scene, shaders, registered);
    for published in scene.set.published() {
        let Some(variant) = WorldVariant::of(published) else {
            continue;
        };
        if !registered.insert(variant.program) {
            continue;
        }
        for family in families {
            let source = published.material.library(family.group);
            let path = format!(
                "src/world_material/{:x}_{:032x}.wesl",
                family.namespace, variant.program
            );
            let _ = shaders.insert(
                variant.shader(family.namespace).id(),
                (family.build)(source, path),
            );
        }
    }
}

/// Registers each published compute's module once, keyed by its program like the materials'
/// modules, which [`register_shaders`] never removes.
fn register_computes(
    scene: &WorldMaterialScene,
    shaders: &mut bevy::asset::Assets<Shader>,
    registered: &mut BTreeSet<u128>,
) {
    for published in &scene.set.computes {
        let program = published.compute.program();
        if !registered.insert(program) {
            continue;
        }
        let _ = shaders.insert(
            cells::shader(program).id(),
            crate::shader_safety::from_wgsl(
                published.compute.module(),
                format!("src/world_material/compute_{program:032x}.wgsl"),
            ),
        );
    }
}

/// Drops the entries of `ids` whose program `gpu` no longer publishes, once per published
/// revision, so pipeline lookups and failure scans cover only live programs; `program` reads an
/// entry's program. A dropped program's pipelines stay in Bevy's cache with their module and
/// are found again if it is published again.
pub(crate) fn prune_pipelines<K, Id>(
    ids: &mut std::collections::HashMap<K, Id>,
    pruned: &mut Option<u64>,
    gpu: &WorldMaterialGpu,
    program: impl Fn(&K) -> u128,
) {
    if *pruned == gpu.revision {
        return;
    }
    *pruned = gpu.revision;
    ids.retain(|key, _| gpu.publishes(program(key)));
}

/// The error a pipeline failed with for good, if it did. A module the cache has not received
/// yet (`ShaderNotLoaded`, `ShaderImportNotYetAvailable`) is retried each frame, so it is not
/// a failure.
pub(crate) fn lasting_error(
    state: &bevy::render::render_resource::CachedPipelineState,
) -> Option<&bevy::shader::ShaderCacheError> {
    use bevy::shader::ShaderCacheError;
    match state {
        bevy::render::render_resource::CachedPipelineState::Err(
            ShaderCacheError::ShaderNotLoaded(_) | ShaderCacheError::ShaderImportNotYetAvailable,
        ) => None,
        bevy::render::render_resource::CachedPipelineState::Err(error) => Some(error),
        _ => None,
    }
}

/// Records each world material pipeline that failed to compile for good, once, and logs it the
/// first time with the error text, so the session's "failed to compile" report can be traced
/// to its cause; its draws keep the base or fallback pipeline, and the main world reports the
/// failure to the session.
pub(crate) fn record_failures(
    cache: &PipelineCache,
    pipelines: impl IntoIterator<Item = (u128, bevy::render::render_resource::CachedRenderPipelineId)>,
    gpu: &WorldMaterialGpu,
    failures: &render_scene::world_material::WorldMaterialFailures,
) {
    for (program, id) in pipelines {
        let Some(error) = lasting_error(cache.get_render_pipeline_state(id)) else {
            continue;
        };
        for key in gpu.keys_of(program) {
            if failures.record(key) {
                bevy::log::warn!(key, %error, "world material pipeline failed to compile");
            }
        }
    }
}

/// Main-world system registering both renderers' modules when the published set changes.
pub(crate) fn register_all_shaders(
    scene: Option<Res<WorldMaterialScene>>,
    shaders: Option<ResMut<bevy::asset::Assets<Shader>>>,
    mut registered: Local<BTreeSet<u128>>,
) {
    let (Some(scene), Some(mut shaders)) = (scene, shaders) else {
        return;
    };
    if !bevy::ecs::change_detection::DetectChanges::is_changed(&scene) {
        return;
    }
    let families = [
        crate::actor_render::world_materials::family(),
        crate::chunk::world_materials::family(),
        crate::chunk::world_materials::model_family(),
        particles::family(),
        crate::shaderpack::atlas::world_sprites::family(),
    ];
    register_shaders(&families, &scene, &mut shaders, &mut registered);
}

/// One material's GPU resources.
struct Entry {
    variant: WorldVariant,
    /// The effects rows of its frame uniform.
    effects: [[f32; 4]; 2],
    /// One more than the material's slot, which its terrain quads carry.
    tag: u8,
    /// The compute whose cells its group binds.
    cells: Option<u64>,
    uniform: Buffer,
    bind_group: BindGroup,
    /// Kept alive with the bind group that samples them.
    _textures: Vec<bevy::render::render_resource::Texture>,
}

/// GPU resources of every published world material, by key, and the slot table actor draws
/// index.
#[derive(Resource, Default)]
pub(crate) struct WorldMaterialGpu {
    revision: Option<u64>,
    entries: HashMap<u64, Entry>,
    slots: Vec<Option<u64>>,
    /// Every published compute's cells and pipeline.
    pub(crate) computes: cells::Computes,
}

impl WorldMaterialGpu {
    /// The variant and bind group of the material in `slot`, once its resources exist.
    pub(crate) fn slot(&self, slot: u16) -> Option<(WorldVariant, &BindGroup)> {
        let key = (*self.slots.get(usize::from(slot))?)?;
        let entry = self.entries.get(&key)?;
        Some((entry.variant, &entry.bind_group))
    }

    /// Whether any published material has resources.
    pub(crate) fn has_materials(&self) -> bool {
        !self.entries.is_empty()
    }

    /// The widest bloom radius and outline, in pixels, of the published materials that draw in
    /// the effects pass; `None` when none does.
    pub(crate) fn effects_reach(&self) -> Option<[f32; 2]> {
        self.entries
            .values()
            .filter(|entry| entry.variant.effects_pass)
            .map(|entry| [entry.effects[1][2], entry.effects[1][0]])
            .reduce(|a, b| [a[0].max(b[0]), a[1].max(b[1])])
    }

    /// Whether any published material draws particles.
    pub(crate) fn has_particles(&self) -> bool {
        self.entries
            .values()
            .any(|entry| entry.variant.features.particles && entry.effects[0][2] > 0.0)
    }

    /// The variant and particle count of the material in `slot`, when it draws particles.
    pub(crate) fn particles(&self, slot: u16) -> Option<(WorldVariant, u32)> {
        let key = (*self.slots.get(usize::from(slot))?)?;
        let entry = self.entries.get(&key)?;
        let count = entry.effects[0][2] as u32;
        (entry.variant.features.particles && count > 0).then_some((entry.variant, count))
    }

    /// Whether any published material reads the scene, so views copy theirs.
    pub(crate) fn reads_scene(&self) -> bool {
        self.entries
            .values()
            .any(|entry| entry.variant.features.scene)
    }

    /// The variant of the material in `slot`.
    pub(crate) fn variant(&self, slot: u16) -> Option<WorldVariant> {
        self.slot(slot).map(|(variant, _)| variant)
    }

    /// The key of the material in `slot`.
    pub(crate) fn material_key(&self, slot: u16) -> Option<u64> {
        (*self.slots.get(usize::from(slot))?).filter(|key| self.entries.contains_key(key))
    }

    /// Whether a material with `program` has resources.
    pub(crate) fn publishes(&self, program: u128) -> bool {
        self.entries
            .values()
            .any(|entry| entry.variant.program == program)
    }

    /// The keys of the materials with `program`.
    pub(crate) fn keys_of(&self, program: u128) -> impl Iterator<Item = u64> + '_ {
        self.entries
            .iter()
            .filter(move |(_, entry)| entry.variant.program == program)
            .map(|(key, _)| *key)
    }
}

/// Creates the resources of newly published materials, drops those of retired ones and
/// writes every frame uniform. wgpu keeps a dropped resource alive until the commands that
/// use it complete, so retirement never invalidates a frame in flight.
pub(crate) fn prepare(
    scene: Option<Res<WorldMaterialScene>>,
    params: Option<Res<render_scene::world_material::WorldMaterialParamScene>>,
    phase: Option<Res<WorldMaterialPhase>>,
    device: Option<Res<RenderDevice>>,
    queue: Option<Res<RenderQueue>>,
    cache: Option<Res<PipelineCache>>,
    gpu: Option<ResMut<WorldMaterialGpu>>,
) {
    let (Some(scene), Some(device), Some(queue), Some(cache), Some(mut gpu)) =
        (scene, device, queue, cache, gpu)
    else {
        return;
    };
    let set = &scene.set;
    if gpu.revision != Some(set.revision) {
        gpu.revision = Some(set.revision);
        let gpu = &mut *gpu;
        gpu.computes.sync(&set.computes, &device, &cache);
        let live: BTreeSet<u64> = set.published().map(|m| m.key).collect();
        gpu.entries.retain(|key, _| live.contains(key));
        let mut slots = Vec::with_capacity(set.materials.len());
        for published in &set.materials {
            let Some(published) = published else {
                slots.push(None);
                continue;
            };
            // A material's group binds its compute's cells, so a new compute needs a new group.
            let cells = published
                .effects
                .cells
                .and_then(|index| gpu.computes.get(index));
            let cells_key = cells.map(|(key, _)| key);
            if gpu
                .entries
                .get(&published.key)
                .is_some_and(|entry| entry.cells != cells_key)
            {
                gpu.entries.remove(&published.key);
            }
            if !gpu.entries.contains_key(&published.key)
                && let Some(entry) = create(&device, &queue, &cache, published, cells)
            {
                gpu.entries.insert(published.key, entry);
            }
            slots.push(
                gpu.entries
                    .contains_key(&published.key)
                    .then_some(published.key),
            );
        }
        gpu.slots = slots;
        for (slot, published) in set.materials.iter().enumerate() {
            if let Some(published) = published
                && let Some(entry) = gpu.entries.get_mut(&published.key)
            {
                entry.effects = published.effects.rows();
                entry.tag = u8::try_from(slot + 1).unwrap_or(0);
            }
        }
    }
    let phase = phase.map_or(0.0, |phase| phase.0);
    let values = params.as_deref().map(|params| &params.params.values);
    for (key, entry) in &gpu.entries {
        let params = values
            .and_then(|values| values.get(key))
            .unwrap_or(&[0.0; 16]);
        let words = abi::frame_uniform_with_effects(phase, entry.tag, entry.effects, params);
        queue.write_buffer(&entry.uniform, 0, bytemuck::cast_slice(&words));
    }
}

/// Uploads one material's textures and builds its bind group, binding `cells`' current cells
/// when its module reads them; such a material waits until they exist.
fn create(
    device: &RenderDevice,
    queue: &RenderQueue,
    cache: &PipelineCache,
    published: &Published,
    cells: Option<(u64, &cells::ComputeEntry)>,
) -> Option<Entry> {
    let variant = WorldVariant::of(published)?;
    let cells = if variant.features.cells {
        Some(cells?)
    } else {
        None
    };
    let uniform = device.create_buffer(&BufferDescriptor {
        label: Some("world material frame"),
        size: abi::FRAME_UNIFORM_BYTES as u64,
        usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let textures: Vec<_> = published
        .textures
        .iter()
        .map(|texture| {
            device.create_texture_with_data(
                queue,
                &TextureDescriptor {
                    label: Some("world material texture"),
                    size: Extent3d {
                        width: texture.width,
                        height: texture.height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: TextureDimension::D2,
                    format: TextureFormat::Rgba8UnormSrgb,
                    usage: TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
                TextureDataOrder::LayerMajor,
                &texture.rgba,
            )
        })
        .collect();
    let views: Vec<_> = textures
        .iter()
        .map(|texture| texture.create_view(&TextureViewDescriptor::default()))
        .collect();
    let sampler = device.create_sampler(&bevy::render::render_resource::SamplerDescriptor {
        label: Some("world material sampler"),
        address_mode_u: AddressMode::Repeat,
        address_mode_v: AddressMode::Repeat,
        mag_filter: FilterMode::Nearest,
        min_filter: FilterMode::Nearest,
        ..Default::default()
    });
    let mut entries = vec![BindGroupEntry {
        binding: 0,
        resource: uniform.as_entire_binding(),
    }];
    for (index, view) in views.iter().enumerate() {
        entries.push(BindGroupEntry {
            binding: index as u32 + 1,
            resource: BindingResource::TextureView(view),
        });
    }
    if !views.is_empty() {
        entries.push(BindGroupEntry {
            binding: views.len() as u32 + 1,
            resource: BindingResource::Sampler(&sampler),
        });
    }
    if let (Some((_, compute)), Some(binding)) = (
        cells,
        abi::MaterialBindings::of(views.len(), variant.features).cells,
    ) {
        entries.push(BindGroupEntry {
            binding,
            resource: compute.current.as_entire_binding(),
        });
    }
    let bind_group = device.create_bind_group(
        Some("world material bind group"),
        &cache.get_bind_group_layout(&variant.layout()),
        &entries,
    );
    Some(Entry {
        variant,
        effects: published.effects.rows(),
        tag: 0,
        cells: cells.map(|(key, _)| key),
        uniform,
        bind_group,
        _textures: textures,
    })
}

/// Runs this frame's compute steps before anything draws, so every view reads the same cells.
pub(crate) fn dispatch_computes(
    gpu: Option<Res<WorldMaterialGpu>>,
    phase: Option<Res<WorldMaterialPhase>>,
    device: Option<Res<RenderDevice>>,
    queue: Option<Res<RenderQueue>>,
    cache: Option<Res<PipelineCache>>,
    mut pending: Option<ResMut<bevy::render::renderer::PendingCommandBuffers>>,
    mut cursor: Local<usize>,
) {
    let (Some(gpu), Some(device), Some(queue), Some(cache), Some(pending)) =
        (gpu, device, queue, cache, pending.as_deref_mut())
    else {
        return;
    };
    if gpu.computes.entries.is_empty() {
        return;
    }
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("world material compute"),
    });
    let phase = phase.map_or(0.0, |phase| phase.0);
    if cells::encode(
        gpu.computes.ordered(),
        phase,
        (&queue, &cache),
        &mut encoder,
        &mut cursor,
    ) > 0
    {
        pending.push_encoder(encoder, "world material compute");
    }
}

/// Records each compute pipeline that failed to compile, once; materials keep reading its
/// zeroed cells.
pub(crate) fn record_compute_failures(
    gpu: Option<Res<WorldMaterialGpu>>,
    cache: Option<Res<PipelineCache>>,
    failures: Option<Res<render_scene::world_material::WorldMaterialFailures>>,
) {
    let (Some(gpu), Some(cache), Some(failures)) = (gpu, cache, failures) else {
        return;
    };
    for (key, entry) in &gpu.computes.entries {
        if matches!(
            cache.get_compute_pipeline_state(entry.pipeline()),
            bevy::render::render_resource::CachedPipelineState::Err(_)
        ) {
            failures.record(*key);
        }
    }
}

/// Installs the shared world material resources once, whichever renderer asks first.
pub(crate) fn install(app: &mut bevy::prelude::App) {
    use bevy::prelude::IntoScheduleConfigs;
    use bevy::render::extract_resource::ExtractResourcePlugin;
    if app.world().contains_resource::<WorldMaterialPhase>() {
        return;
    }
    app.init_resource::<WorldMaterialScene>()
        .init_resource::<render_scene::world_material::WorldMaterialParamScene>()
        .init_resource::<WorldMaterialPhase>()
        .init_resource::<render_scene::world_material::WorldMaterialFailures>()
        .init_resource::<render_scene::world_material::WorldMaterialCarried>()
        .add_plugins((
            ExtractResourcePlugin::<WorldMaterialScene>::default(),
            ExtractResourcePlugin::<render_scene::world_material::WorldMaterialParamScene>::default(
            ),
            ExtractResourcePlugin::<WorldMaterialPhase>::default(),
        ));
    // Renderer-only test apps install no main schedules.
    if app.get_schedule(bevy::prelude::PostUpdate).is_some() {
        app.add_systems(
            bevy::prelude::PostUpdate,
            (advance_phase, register_all_shaders),
        );
    }
    let failures = app
        .world()
        .resource::<render_scene::world_material::WorldMaterialFailures>()
        .clone();
    let carried = app
        .world()
        .resource::<render_scene::world_material::WorldMaterialCarried>()
        .clone();
    if let Some(render_app) = app.get_sub_app_mut(bevy::render::RenderApp) {
        render_app
            .init_resource::<WorldMaterialGpu>()
            .init_resource::<scene::WorldSceneCopies>()
            .insert_resource(failures)
            .insert_resource(carried)
            .add_systems(
                bevy::render::Render,
                (
                    prepare.in_set(bevy::render::RenderSystems::PrepareResources),
                    (dispatch_computes, record_compute_failures)
                        .after(prepare)
                        .in_set(bevy::render::RenderSystems::PrepareResources),
                    scene::prepare
                        .in_set(bevy::render::RenderSystems::PrepareBindGroups)
                        .after(bevy::render::view::prepare_view_targets),
                    effects_pass::prepare
                        .in_set(bevy::render::RenderSystems::PrepareBindGroups)
                        .after(bevy::render::view::prepare_view_targets),
                    (
                        crate::actor_render::world_effects::gather_particles,
                        crate::chunk::gather_block_particles,
                        particles::prepare,
                    )
                        .chain()
                        .after(prepare)
                        .in_set(bevy::render::RenderSystems::PrepareResources),
                ),
            )
            .init_resource::<effects_pass::WorldEffectsViews>()
            .init_resource::<particles::ParticleTargets>()
            .init_resource::<particles::WorldParticles>();
        let _ = render_app.world_mut().try_schedule_scope(
            bevy::core_pipeline::Core3d,
            |_, schedule| {
                schedule.add_systems((
                    scene::copy_scene
                        .after(crate::scene_target::ScenePass::Opaque)
                        .after(crate::entity_shadow_render::EntityShadowLabel)
                        .after(crate::chunk::GpuCullLateLabel)
                        .before(crate::scene_target::ScenePass::Transparent)
                        .in_set(bevy::core_pipeline::Core3dSystems::MainPass),
                    (particles::draw, effects_pass::draw)
                        .chain()
                        .after(crate::scene_target::ScenePass::Transparent)
                        .before(crate::scene_target::ScenePass::Finish)
                        .in_set(bevy::core_pipeline::Core3dSystems::MainPass),
                ));
            },
        );
    }
}

pub(crate) mod cells;
pub(crate) mod effects_pass;
pub(crate) mod particles;
pub(crate) mod scene;

#[cfg(test)]
mod tests;
