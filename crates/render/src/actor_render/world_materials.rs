//! Programmable world materials on actor draws: a fragment shader per material, composed from
//! the actor shader and the guest's library, with its textures, parameters and phase in bind
//! group [`WORLD_MATERIAL_GROUP`]. Draws whose material is not ready use the base pipeline.

use super::*;
use bevy::asset::uuid::Uuid;
use bevy::render::{
    render_resource::{TextureDataOrder, TextureDimension, TextureFormat, TextureUsages},
    renderer::RenderQueue,
};
use mod_render::world_material::{
    self as abi,
    publication::{Blend, Published},
};
use render_scene::world_material::WorldMaterialScene;
use std::collections::{BTreeSet, HashMap};

/// The bind group a world material pipeline reads its frame, textures and sampler from.
pub(crate) const WORLD_MATERIAL_GROUP: u32 = 2;
/// Keeps world material shader ids apart from every other runtime shader.
const SHADER_NAMESPACE: u64 = u64::from_le_bytes(*b"wmactor\0");
/// The actor shader every world material's fragment module extends.
const ACTOR_SOURCE: &str = include_str!("../actor.wesl");

/// What distinguishes a world material's actor pipelines from the base ones.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct WorldVariant {
    pub key: u64,
    pub textures: u8,
    pub blend: Blend,
}

impl WorldVariant {
    fn of(published: &Published) -> Option<Self> {
        Some(Self {
            key: published.key,
            textures: u8::try_from(published.textures.len()).ok()?,
            blend: published.blend,
        })
    }

    /// The fragment shader of this material.
    pub(crate) fn shader(self) -> Handle<Shader> {
        Handle::Uuid(
            Uuid::from_u64_pair(SHADER_NAMESPACE, self.key),
            std::marker::PhantomData,
        )
    }

    /// Points the fragment stage at this material's module and adds its bind group.
    pub(crate) fn specialize(self, descriptor: &mut RenderPipelineDescriptor) {
        descriptor.layout.truncate(WORLD_MATERIAL_GROUP as usize);
        descriptor.layout.push(layout(self.textures));
        let fragment = descriptor
            .fragment
            .as_mut()
            .expect("actor pipelines draw colour");
        fragment.shader = self.shader();
        let defs = [
            ("WORLD_MATERIAL", true),
            ("WORLD_MATERIAL_CUTOUT", self.blend == Blend::Cutout),
            ("WORLD_MATERIAL_OPAQUE", self.blend == Blend::Opaque),
            (
                "WORLD_MATERIAL_PREMULTIPLY",
                matches!(self.blend, Blend::Premultiplied | Blend::Additive),
            ),
        ];
        for (name, enabled) in defs {
            fragment
                .shader_defs
                .push(bevy::shader::ShaderDefVal::Bool(name.into(), enabled));
        }
    }
}

/// Bind group layout of a material with `textures` declared textures: the frame uniform, the
/// textures, then a sampler when there is one.
pub(crate) fn layout(textures: u8) -> BindGroupLayoutDescriptor {
    let mut entries = vec![BindGroupLayoutEntry {
        binding: 0,
        visibility: ShaderStages::FRAGMENT,
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

/// Advances the phase from real time, as drawing materials' does.
pub(crate) fn advance_phase(
    time: Option<Res<bevy::time::Time<bevy::time::Real>>>,
    mut phase: ResMut<WorldMaterialPhase>,
) {
    let Some(time) = time else {
        return;
    };
    let next = mod_render::material::phase(time.elapsed_secs_f64());
    if phase.0 != next {
        phase.0 = next;
    }
}

/// Registers each published material's composed actor shader, and removes those no longer
/// published so a revoked material's module is not retained.
pub(crate) fn register_shaders(
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
    let live: BTreeSet<u64> = scene.set.materials.iter().map(|m| m.key).collect();
    for published in &scene.set.materials {
        let Some(variant) = WorldVariant::of(published) else {
            continue;
        };
        if !registered.insert(published.key) {
            continue;
        }
        let source = format!(
            "{ACTOR_SOURCE}\n{}",
            published.material.library(WORLD_MATERIAL_GROUP)
        );
        let shader = crate::shader_safety::from_actor_wesl(
            &source,
            format!("src/world_material/actor_{}.wesl", published.key),
            render_model::ACTOR_GPU_INSTANCE_WORDS,
            render_model::ACTOR_RIG_VERTEX_WORDS,
        );
        let _ = shaders.insert(variant.shader().id(), shader);
    }
    registered.retain(|key| {
        let keep = live.contains(key);
        if !keep {
            shaders.remove(
                Handle::<Shader>::Uuid(
                    Uuid::from_u64_pair(SHADER_NAMESPACE, *key),
                    std::marker::PhantomData,
                )
                .id(),
            );
        }
        keep
    });
}

/// One material's GPU resources.
struct Entry {
    variant: WorldVariant,
    params: [f32; 16],
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
        for published in &set.materials {
            if let Some(entry) = gpu.entries.get_mut(&published.key) {
                entry.params = published.params;
            }
        }
    }
    let phase = phase.map_or(0.0, |phase| phase.0);
    for entry in gpu.entries.values() {
        let words = abi::frame_uniform(phase, &entry.params);
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
        uniform,
        bind_group,
        _textures: textures,
    })
}

/// A base actor pipeline key drawn with a world material's fragment module.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct WorldActorKey {
    pub(super) base: ActorPipelineKey,
    pub(super) world: WorldVariant,
}

impl WorldActorKey {
    /// The guest decides coverage, so the Bedrock material's alpha test and alpha to coverage
    /// do not apply; the raster stays the host's class raster.
    pub(super) fn new(mut base: ActorPipelineKey, world: WorldVariant) -> Self {
        base.material &= !(assets::EntityRenderMaterialState::ALPHA_TEST
            | render_model::actor::frame::ACTOR_WORLD_MATERIAL_MASK);
        base.raster.alpha_to_coverage = false;
        Self { base, world }
    }
}

impl SpecializerKey for WorldActorKey {
    const IS_CANONICAL: bool = false;
    type Canonical = (ActorPipelineContract, WorldVariant);
}

pub(super) struct WorldActorSpecializer;

impl Specializer<RenderPipeline> for WorldActorSpecializer {
    type Key = WorldActorKey;

    fn specialize(
        &self,
        key: Self::Key,
        descriptor: &mut RenderPipelineDescriptor,
    ) -> Result<Canonical<Self::Key>, BevyError> {
        let contract = ActorPipelineSpecializer.specialize(key.base, descriptor)?;
        descriptor.label = Some("world material actor pipeline".into());
        key.world.specialize(descriptor);
        Ok((contract, key.world))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> ActorPipelineKey {
        ActorPipelineKey {
            msaa: Msaa::Off,
            hdr: false,
            enhanced: false,
            material: assets::EntityRenderMaterial::Default as u32
                | assets::EntityRenderMaterialState::AUTHORED
                | assets::EntityRenderMaterialState::ALPHA_TEST
                | (3 << render_model::actor::frame::ACTOR_WORLD_MATERIAL_SHIFT),
            raster: render_scene::world_material::host_raster(
                mod_render::world_material::publication::Depth::Test,
                Blend::Premultiplied,
            ),
        }
    }

    #[test]
    fn world_pipelines_swap_only_the_fragment_module_and_add_their_bind_group() {
        let variant = WorldVariant {
            key: 7,
            textures: 2,
            blend: Blend::Premultiplied,
        };
        let key = WorldActorKey::new(base(), variant);
        assert_eq!(
            key.base.material & assets::EntityRenderMaterialState::ALPHA_TEST,
            0,
            "the guest decides coverage"
        );
        let mut descriptor = actor_pipeline_descriptor(actor_bind_group_layout());
        let (_, world) = WorldActorSpecializer
            .specialize(key, &mut descriptor)
            .unwrap();
        assert_eq!(world, variant);
        assert_eq!(descriptor.layout.len(), WORLD_MATERIAL_GROUP as usize + 1);
        assert_eq!(descriptor.layout[2], layout(2));
        let fragment = descriptor.fragment.as_ref().unwrap();
        assert_eq!(fragment.shader, variant.shader());
        assert_eq!(descriptor.vertex.shader, ACTOR_SHADER_HANDLE);
        let enabled = |name: &'static str| {
            fragment
                .shader_defs
                .iter()
                .any(|def| *def == bevy::shader::ShaderDefVal::Bool(name.into(), true))
        };
        assert!(enabled("WORLD_MATERIAL") && enabled("WORLD_MATERIAL_PREMULTIPLY"));
        assert!(!enabled("WORLD_MATERIAL_CUTOUT"));
        // The host's class raster blends; the guest chose no factors.
        let target = fragment.targets[0].as_ref().unwrap();
        assert!(target.blend.is_some());
        assert_eq!(
            descriptor
                .depth_stencil
                .as_ref()
                .unwrap()
                .depth_write_enabled,
            Some(false)
        );
        // Distinct materials never share a pipeline.
        let other = WorldVariant { key: 8, ..variant };
        assert_ne!(other.shader(), variant.shader());
    }
}
