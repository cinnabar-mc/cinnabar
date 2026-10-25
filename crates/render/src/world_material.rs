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

/// What distinguishes a world material's pipelines from the base ones.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct WorldVariant {
    pub key: u64,
    pub textures: u8,
    pub blend: Blend,
    pub depth: Depth,
}

impl WorldVariant {
    pub(crate) fn of(published: &Published) -> Option<Self> {
        Some(Self {
            key: published.key,
            textures: u8::try_from(published.textures.len()).ok()?,
            blend: published.blend,
            depth: published.depth,
        })
    }

    /// This material's module in the renderer `namespace`.
    pub(crate) fn shader(self, namespace: u64) -> Handle<Shader> {
        shader(namespace, self.key)
    }

    /// The shader definitions selecting the host's handling of the material's blend class.
    pub(crate) fn defs(self) -> [bevy::shader::ShaderDefVal; 4] {
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
        ]
    }
}

/// The module of material `key` in renderer `namespace`.
fn shader(namespace: u64, key: u64) -> Handle<Shader> {
    Handle::Uuid(
        Uuid::from_u64_pair(namespace, key),
        std::marker::PhantomData,
    )
}

/// Bind group layout of a material with `textures` declared textures: the frame uniform, the
/// textures, then a sampler when there is one.
pub(crate) fn layout(textures: u8) -> BindGroupLayoutDescriptor {
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
pub(crate) fn advance_phase(mut phase: ResMut<WorldMaterialPhase>) {
    let Ok(since_epoch) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) else {
        return;
    };
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

/// Registers each published material's composed modules for every renderer, and removes those
/// no longer published so a revoked material's modules are not retained.
pub(crate) fn register_shaders(
    families: &[ShaderFamily],
    scene: &WorldMaterialScene,
    shaders: &mut bevy::asset::Assets<Shader>,
    registered: &mut BTreeSet<u64>,
) {
    let live: BTreeSet<u64> = scene.set.materials.iter().map(|m| m.key).collect();
    for published in &scene.set.materials {
        if WorldVariant::of(published).is_none() || !registered.insert(published.key) {
            continue;
        }
        for family in families {
            let source = published.material.library(family.group);
            let path = format!(
                "src/world_material/{:x}_{}.wesl",
                family.namespace, published.key
            );
            let _ = shaders.insert(
                shader(family.namespace, published.key).id(),
                (family.build)(source, path),
            );
        }
    }
    registered.retain(|key| {
        let keep = live.contains(key);
        if !keep {
            for family in families {
                shaders.remove(shader(family.namespace, *key).id());
            }
        }
        keep
    });
}

/// Records each world material pipeline that failed to compile, once; its draws keep the base or
/// fallback pipeline, and the main world reports the failure to the session.
pub(crate) fn record_failures(
    cache: &PipelineCache,
    pipelines: impl IntoIterator<Item = (u64, bevy::render::render_resource::CachedRenderPipelineId)>,
    failures: &render_scene::world_material::WorldMaterialFailures,
) {
    for (key, id) in pipelines {
        if matches!(
            cache.get_render_pipeline_state(id),
            bevy::render::render_resource::CachedPipelineState::Err(_)
        ) {
            failures.record(key);
        }
    }
}

/// Main-world system registering both renderers' modules when the published set changes.
pub(crate) fn register_all_shaders(
    scene: Option<Res<WorldMaterialScene>>,
    shaders: Option<ResMut<bevy::asset::Assets<Shader>>>,
    mut registered: Local<BTreeSet<u64>>,
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
        crate::shaderpack::atlas::world_sprites::family(),
    ];
    register_shaders(&families, &scene, &mut shaders, &mut registered);
}

/// One material's GPU resources.
struct Entry {
    variant: WorldVariant,
    params: [f32; 16],
    /// One more than the material's slot, which its terrain quads carry.
    tag: u8,
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

    /// The variant of the material in `slot`.
    pub(crate) fn variant(&self, slot: u16) -> Option<WorldVariant> {
        self.slot(slot).map(|(variant, _)| variant)
    }
}

/// Creates the resources of newly published materials, drops those of retired ones and
/// writes every frame uniform. wgpu keeps a dropped resource alive until the commands that
/// use it complete, so retirement never invalidates a frame in flight.
pub(crate) fn prepare(
    scene: Option<Res<WorldMaterialScene>>,
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
        let live: BTreeSet<u64> = set.materials.iter().map(|m| m.key).collect();
        gpu.entries.retain(|key, _| live.contains(key));
        let mut slots = Vec::with_capacity(set.materials.len());
        for published in &set.materials {
            if !gpu.entries.contains_key(&published.key)
                && let Some(entry) = create(&device, &queue, &cache, published)
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
            if let Some(entry) = gpu.entries.get_mut(&published.key) {
                entry.params = published.params;
                entry.tag = u8::try_from(slot + 1).unwrap_or(0);
            }
        }
    }
    let phase = phase.map_or(0.0, |phase| phase.0);
    for entry in gpu.entries.values() {
        let words = abi::frame_uniform(phase, entry.tag, &entry.params);
        queue.write_buffer(&entry.uniform, 0, bytemuck::cast_slice(&words));
    }
}

/// Uploads one material's textures and builds its bind group.
fn create(
    device: &RenderDevice,
    queue: &RenderQueue,
    cache: &PipelineCache,
    published: &Published,
) -> Option<Entry> {
    let variant = WorldVariant::of(published)?;
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
    let bind_group = device.create_bind_group(
        Some("world material bind group"),
        &cache.get_bind_group_layout(&layout(variant.textures)),
        &entries,
    );
    Some(Entry {
        variant,
        params: published.params,
        tag: 0,
        uniform,
        bind_group,
        _textures: textures,
    })
}

/// Installs the shared world material resources once, whichever renderer asks first.
pub(crate) fn install(app: &mut bevy::prelude::App) {
    use bevy::prelude::IntoScheduleConfigs;
    use bevy::render::extract_resource::ExtractResourcePlugin;
    if app.world().contains_resource::<WorldMaterialPhase>() {
        return;
    }
    app.init_resource::<WorldMaterialScene>()
        .init_resource::<WorldMaterialPhase>()
        .init_resource::<render_scene::world_material::WorldMaterialFailures>()
        .add_plugins((
            ExtractResourcePlugin::<WorldMaterialScene>::default(),
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
    if let Some(render_app) = app.get_sub_app_mut(bevy::render::RenderApp) {
        render_app
            .init_resource::<WorldMaterialGpu>()
            .insert_resource(failures)
            .add_systems(
                bevy::render::Render,
                prepare.in_set(bevy::render::RenderSystems::PrepareResources),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mod_render::world_material::compose;

    /// A scene publishing one plain material with a texture, bound to one entity.
    fn scene() -> WorldMaterialScene {
        let mut scene = WorldMaterialScene::with_bound_entities(&["example:moth"]);
        let set = std::sync::Arc::make_mut(&mut scene.set);
        set.materials[0].textures = std::sync::Arc::from([abi::publication::Texture {
            width: 2,
            height: 2,
            rgba: std::sync::Arc::from([255_u8; 16]),
        }]);
        scene
    }

    #[test]
    fn revocation_removes_every_renderers_modules() {
        let mut shaders = bevy::asset::Assets::<Shader>::default();
        let mut registered = BTreeSet::new();
        let families = [
            crate::actor_render::world_materials::family(),
            crate::chunk::world_materials::family(),
        ];
        let published = scene();
        register_shaders(&families, &published, &mut shaders, &mut registered);
        let key = published.set.materials[0].key;
        for family in &families {
            assert!(shaders.get(shader(family.namespace, key).id()).is_some());
        }
        register_shaders(
            &families,
            &WorldMaterialScene::default(),
            &mut shaders,
            &mut registered,
        );
        for family in &families {
            assert!(shaders.get(shader(family.namespace, key).id()).is_none());
        }
        assert!(registered.is_empty());
    }

    #[test]
    fn revocation_retains_no_gpu_resources() {
        use bevy::ecs::system::RunSystemOnce;
        let (mut app, _) = crate::queue_review_support::app();
        let world = app.world_mut();
        world.init_resource::<WorldMaterialGpu>();
        world.insert_resource(scene());
        world.run_system_once(prepare).unwrap();
        let gpu = world.resource::<WorldMaterialGpu>();
        assert!(gpu.has_materials() && gpu.slot(0).is_some());
        world.insert_resource(WorldMaterialScene::default());
        world.run_system_once(prepare).unwrap();
        let gpu = world.resource::<WorldMaterialGpu>();
        assert!(!gpu.has_materials() && gpu.slot(0).is_none());
    }

    /// The production actor and cube modules compose and validate with a world material
    /// library in every blend class, as Bevy's WESL composition builds them.
    #[test]
    fn world_material_modules_compose_for_actors_and_terrain() {
        let material = compose(
            "fn shade(input: WorldMaterialInput) -> Surface {
                let glow = texture(1u, input.uv).rgb * param(input, 3u);
                return Surface(texture(0u, input.uv).rgb * input.light.rgb, input.base.a, glow);
            }",
            2,
        )
        .unwrap();
        let actor = format!(
            "{}\n{}",
            include_str!("actor.wesl")
                .replace(
                    "ACTOR_GPU_INSTANCE_WORDS",
                    &format!("{}u", render_model::ACTOR_GPU_INSTANCE_WORDS),
                )
                .replace(
                    "ACTOR_RIG_VERTEX_WORDS",
                    &format!("{}u", render_model::ACTOR_RIG_VERTEX_WORDS),
                ),
            material.library(crate::actor_render::world_materials::WORLD_MATERIAL_GROUP)
        );
        let terrain = crate::material_shader::source(&format!(
            "{}\n{}",
            include_str!("chunk.wesl"),
            material.library(crate::chunk::world_materials::WORLD_MATERIAL_GROUP)
        ));
        let model = crate::material_shader::source(&format!(
            "{}\n{}",
            include_str!("model.wesl"),
            material.library(crate::chunk::world_materials::WORLD_MATERIAL_GROUP)
        ));
        for class in [
            "WORLD_MATERIAL_OPAQUE",
            "WORLD_MATERIAL_CUTOUT",
            "WORLD_MATERIAL_PREMULTIPLY",
        ] {
            crate::shader_source::composed(&actor, &["WORLD_MATERIAL", class]);
            crate::shader_source::composed(&terrain, &["WORLD_MATERIAL", class]);
            crate::shader_source::composed(&model, &["WORLD_MATERIAL", class]);
        }
        // Translucent classes also compose for the gamma-blended translucent pass.
        for module in [&terrain, &model] {
            crate::shader_source::composed(
                module,
                &[
                    "WORLD_MATERIAL",
                    "WORLD_MATERIAL_PREMULTIPLY",
                    "NATIVE_GAMMA_BLEND",
                ],
            );
        }
        // The Bedrock fallback passes compose without any library.
        for source in [include_str!("chunk.wesl"), include_str!("model.wesl")] {
            crate::shader_source::composed(
                &crate::material_shader::source(source),
                &["WORLD_MATERIAL_FALLBACK"],
            );
        }
    }

    /// The phase follows the wall clock, so a server timing an effect by Unix time agrees.
    #[test]
    fn the_phase_follows_the_wall_clock() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = bevy::prelude::World::new();
        world.init_resource::<WorldMaterialPhase>();
        let wall = || {
            let since = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs_f64();
            mod_render::material::phase(since)
        };
        let before = wall();
        world.run_system_once(advance_phase).unwrap();
        let after = wall();
        let phase = world.resource::<WorldMaterialPhase>().0;
        // Within the two readings, allowing for the wrap between them.
        let wrapped = after < before;
        assert!(
            (phase >= before && phase <= after) || (wrapped && (phase >= before || phase <= after)),
            "{before} <= {phase} <= {after}"
        );
    }
}
