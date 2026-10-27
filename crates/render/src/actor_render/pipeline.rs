use super::*;
use render_model::{ActorGpuInstance, ActorRigGeometrySpan};
use std::collections::hash_map::Entry;

pub(super) struct ActorPipelineSpecializer;

#[derive(Resource)]
pub(crate) struct ActorPipeline {
    pub(super) compact: bool,
    pub(super) motion_supported: bool,
    pub(super) variants: Variants<RenderPipeline, ActorPipelineSpecializer>,
    pub(super) bind_group_layout: BindGroupLayoutDescriptor,
    draw_variants: std::collections::HashMap<
        ActorPipelineContract,
        bevy::render::render_resource::CachedRenderPipelineId,
    >,
    motion_variants: Variants<RenderPipeline, super::motion::MotionSpecializer>,
    motion_ids: std::collections::HashMap<ActorPipelineContract, CachedRenderPipelineId>,
    /// Unique draw and motion pipelines for each view, retained for readiness and shader reloads.
    prewarmed: std::collections::HashMap<(Msaa, bool, bool), Vec<CachedRenderPipelineId>>,
    /// The session's published authored rasters and the views that already requested them.
    session_rasters: (AuthoredRasters, Vec<(Msaa, bool, bool)>),
}

/// Authored raster states by material index, as the session publishes them.
type AuthoredRasters = std::sync::Arc<[(u32, render_api::render_state::RenderState)]>;

impl FromWorld for ActorPipeline {
    fn from_world(world: &mut World) -> Self {
        let limit = world
            .get_resource::<RenderDevice>()
            .map_or(u32::MAX, |device| {
                device.limits().max_storage_buffers_per_shader_stage
            });
        let compact =
            limit < crate::device_settings::vertex_storage_buffers(&actor_bind_group_layout());
        let mut bind_group_layout = actor_bind_group_layout();
        if compact {
            bind_group_layout.entries.retain(|entry| entry.binding != 5);
        }
        let descriptor = actor_pipeline_descriptor(bind_group_layout.clone());
        let mut motion_descriptor = descriptor.clone();
        motion_descriptor.layout.push(super::motion::layout());
        Self {
            compact,
            motion_supported: limit
                >= crate::device_settings::vertex_storage_buffers(&actor_bind_group_layout())
                    + crate::device_settings::vertex_storage_buffers(&super::motion::layout()),
            motion_variants: Variants::new(super::motion::MotionSpecializer, motion_descriptor),
            motion_ids: Default::default(),
            prewarmed: Default::default(),
            session_rasters: (std::sync::Arc::from([]), Vec::new()),
            variants: Variants::new(ActorPipelineSpecializer, descriptor),
            bind_group_layout,
            draw_variants: Default::default(),
        }
    }
}

impl ActorPipeline {
    /// Removes the separate current endpoint binding when both endpoints share one allocation.
    pub(super) fn entries<'a, 'b>(
        &self,
        entries: &'b [BindGroupEntry<'a>],
    ) -> std::borrow::Cow<'b, [BindGroupEntry<'a>]> {
        if self.compact {
            std::borrow::Cow::Owned(
                entries
                    .iter()
                    .filter(|entry| entry.binding != 5)
                    .cloned()
                    .collect(),
            )
        } else {
            std::borrow::Cow::Borrowed(entries)
        }
    }

    /// Queues draw and eligible motion contracts before actors enter a view.
    pub(super) fn prewarm(
        &mut self,
        cache: &PipelineCache,
        msaa: Msaa,
        hdr: bool,
        enhanced: bool,
    ) -> Option<bevy::render::render_resource::CachedRenderPipelineId> {
        let view = (msaa, hdr, enhanced);
        if !self.prewarmed.contains_key(&view) {
            let mut ids = Vec::new();
            for (material, raster) in prewarm_materials() {
                let key = ActorPipelineKey {
                    msaa,
                    hdr,
                    enhanced,
                    material,
                    raster,
                };
                let contract = key.contract();
                let id = match self.draw_variants.entry(contract) {
                    Entry::Occupied(entry) => *entry.get(),
                    Entry::Vacant(entry) => {
                        *entry.insert(self.variants.specialize(cache, key).ok()?)
                    }
                };
                if !ids.contains(&id) {
                    ids.push(id);
                }
                if self.motion_supported && super::motion::eligible(material, &key.raster()) {
                    let id = match self.motion_ids.entry(contract) {
                        Entry::Occupied(entry) => *entry.get(),
                        Entry::Vacant(entry) => {
                            *entry.insert(self.motion_variants.specialize(cache, key).ok()?)
                        }
                    };
                    if !ids.contains(&id) {
                        ids.push(id);
                    }
                }
            }
            // A failed specialization returned above, so it retries next frame as before.
            self.prewarmed.insert(view, ids);
        }
        self.draw_variant(
            msaa,
            hdr,
            enhanced,
            assets::EntityRenderMaterial::Default as u32,
            assets::entity::material::UNAUTHORED_RENDER_STATE,
        )
    }

    /// Looks up a draw contract already requested by prewarming or the frame's raster table.
    pub(super) fn draw_variant(
        &self,
        msaa: Msaa,
        hdr: bool,
        enhanced: bool,
        material: u32,
        raster: render_api::render_state::RenderState,
    ) -> Option<bevy::render::render_resource::CachedRenderPipelineId> {
        self.draw_variants
            .get(
                &ActorPipelineKey {
                    msaa,
                    hdr,
                    enhanced,
                    material,
                    raster,
                }
                .contract(),
            )
            .copied()
    }

    /// Queues draw and, where the device supports motion, motion pipelines for raster states a
    /// frame supplies, so authored definitions compile while their actors load rather than on
    /// first draw.
    pub(super) fn prewarm_rasters(
        &mut self,
        cache: &PipelineCache,
        view: (Msaa, bool, bool),
        materials: impl IntoIterator<Item = (u32, render_api::render_state::RenderState)>,
    ) -> Option<()> {
        let (msaa, hdr, enhanced) = view;
        for (material, raster) in materials {
            let key = ActorPipelineKey {
                msaa,
                hdr,
                enhanced,
                material,
                raster,
            };
            let contract = key.contract();
            let id = match self.draw_variants.entry(contract) {
                Entry::Occupied(entry) => *entry.get(),
                Entry::Vacant(entry) => *entry.insert(self.variants.specialize(cache, key).ok()?),
            };
            let ids = self.prewarmed.entry(view).or_default();
            if !ids.contains(&id) {
                ids.push(id);
            }
            if self.motion_supported && super::motion::eligible(material, &raster) {
                let id = match self.motion_ids.entry(contract) {
                    Entry::Occupied(entry) => *entry.get(),
                    Entry::Vacant(entry) => {
                        *entry.insert(self.motion_variants.specialize(cache, key).ok()?)
                    }
                };
                let ids = self.prewarmed.entry(view).or_default();
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
        Some(())
    }

    /// Looks up a motion contract already requested by the loading path.
    pub(super) fn motion_variant(&self, key: ActorPipelineKey) -> Option<CachedRenderPipelineId> {
        self.motion_ids.get(&key.contract()).copied()
    }

    /// Loading completes only when both color and eligible motion pipelines are compiled.
    pub(super) fn ready(
        &self,
        cache: &PipelineCache,
        msaa: Msaa,
        hdr: bool,
        enhanced: bool,
    ) -> bool {
        self.ready_with((msaa, hdr, enhanced), |id| {
            cache.get_render_pipeline(id).is_some()
        })
    }

    /// Polls each distinct pipeline once, including pipelines invalidated by a shader reload.
    pub(super) fn ready_with(
        &self,
        view: (Msaa, bool, bool),
        mut compiled: impl FnMut(CachedRenderPipelineId) -> bool,
    ) -> bool {
        self.prewarmed
            .get(&view)
            .is_some_and(|ids| ids.iter().copied().all(&mut compiled))
    }
}

#[cfg(test)]
#[path = "pipeline_readiness_tests.rs"]
mod readiness_tests;

/// Requests every raster contract built-in actor materials draw with before the first actor
/// reaches a view: both cullings, opaque, alpha and additive blending, both depth writes, the
/// default, equal or always-passing depth test and every channel-write mask they declare, with
/// and without alpha testing.
fn prewarm_materials() -> impl Iterator<Item = (u32, render_api::render_state::RenderState)> {
    use render_api::render_state::{BlendFactor, BlendState, CompareFunction, CullMode};
    let kinds = [
        assets::EntityRenderMaterial::Default,
        assets::EntityRenderMaterial::DissolveDepth,
        assets::EntityRenderMaterial::DissolveColor,
    ];
    let blends = [
        None,
        Some(BlendState::ALPHA),
        Some(crate::actor::material::additive(BlendFactor::One)),
        Some(crate::actor::material::additive(BlendFactor::SourceAlpha)),
    ];
    let rasters = [CullMode::None, CullMode::Back]
        .into_iter()
        .flat_map(move |cull| {
            blends.into_iter().flat_map(move |blend| {
                [false, true].into_iter().flat_map(move |write| {
                    [
                        CompareFunction::LessEqual,
                        CompareFunction::Equal,
                        CompareFunction::Always,
                    ]
                    .into_iter()
                    .flat_map(move |compare| {
                        [(true, true), (true, false), (false, false)].map(
                            move |(write_color, write_alpha)| {
                                let mut raster = assets::entity::material::UNAUTHORED_RENDER_STATE;
                                raster.cull = cull;
                                raster.blend = blend;
                                raster.depth.write = write;
                                raster.depth.compare = Some(compare);
                                raster.write_color = write_color;
                                raster.write_alpha = write_alpha;
                                raster
                            },
                        )
                    })
                })
            })
        });
    rasters.flat_map(move |raster| {
        kinds.into_iter().flat_map(move |kind| {
            [false, true]
                .into_iter()
                .map(move |alpha_test| {
                    kind.word(Some(assets::EntityRenderMaterialState {
                        alpha_test,
                        ..Default::default()
                    }))
                })
                .flat_map(|material| {
                    [
                        material,
                        material
                            | render_model::LEGACY_COMBAT_MATERIAL_FLAG
                            | render_model::ACTOR_COLOR_MASK_MATERIAL_FLAG,
                    ]
                })
                .flat_map(|material| {
                    [
                        Some(material),
                        (material & assets::EntityRenderMaterialState::KIND_MASK
                            == assets::EntityRenderMaterial::DissolveColor as u32)
                            .then_some(material | crate::actor::material::LATE_DISSOLVE_COLOR),
                    ]
                    .into_iter()
                    .flatten()
                })
                .map(move |material| (material, raster))
        })
    })
}

pub(super) fn prepare_actor_pipelines(
    cache: Res<PipelineCache>,
    mut pipeline: ResMut<ActorPipeline>,
    readiness: Res<render_scene::ActorPipelineReadiness>,
    views: Query<(
        &bevy::render::camera::ExtractedCamera,
        &Msaa,
        Option<&render_scene::EnhancedRendering>,
    )>,
    frame: Option<Res<render_scene::ActorRenderFrame>>,
) {
    if let Some(frame) = &frame
        && !std::sync::Arc::ptr_eq(&pipeline.session_rasters.0, &frame.authored_rasters)
    {
        pipeline.session_rasters = (std::sync::Arc::clone(&frame.authored_rasters), Vec::new());
    }
    let authored = frame
        .filter(bevy::ecs::change_detection::DetectChanges::is_changed)
        .map(|frame| authored_materials(&frame.rig))
        .unwrap_or_default();
    let mut has_view = false;
    let mut ready = true;
    for (camera, msaa, enhanced) in &views {
        has_view = true;
        for msaa in crate::pipeline_warmup::sample_modes(*msaa) {
            let view = (msaa, camera.hdr, enhanced.is_some());
            // A session's authored materials compile once per view, when its pack loads.
            if !pipeline.session_rasters.1.contains(&view) {
                let session = std::sync::Arc::clone(&pipeline.session_rasters.0);
                if pipeline
                    .prewarm_rasters(&cache, view, session.iter().copied())
                    .is_some()
                {
                    pipeline.session_rasters.1.push(view);
                }
            }
            ready &= pipeline
                .prewarm(&cache, msaa, camera.hdr, enhanced.is_some())
                .is_some()
                && pipeline
                    .prewarm_rasters(&cache, view, authored.iter().copied())
                    .is_some()
                && pipeline.ready(&cache, msaa, camera.hdr, enhanced.is_some());
        }
    }
    readiness.publish(has_view && ready);
}

/// Distinct (word, raster) pairs a frame draws; pairs already requested cost one lookup.
fn authored_materials(
    rig: &render_model::ActorRigRenderFrame,
) -> Vec<(u32, render_api::render_state::RenderState)> {
    let pairs = rig
        .instances
        .iter()
        .map(|instance| (instance.material, instance.raster))
        .collect::<std::collections::BTreeSet<_>>();
    pairs
        .into_iter()
        .filter_map(|(material, raster)| Some((material, *rig.rasters.get(raster as usize)?)))
        .collect()
}

pub(super) fn actor_bind_group_layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "instanced actor bind group layout",
        &[
            BindGroupLayoutEntry {
                binding: 0,
                // The fragment stage reads the camera position for distance fog.
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
                visibility: ShaderStages::VERTEX | ShaderStages::FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(size_of::<ActorGpuInstance>() as u64),
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 2,
                visibility: ShaderStages::VERTEX,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(size_of::<ActorRigVertex>() as u64),
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 3,
                visibility: ShaderStages::VERTEX,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(size_of::<ActorRigGeometrySpan>() as u64),
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 4,
                visibility: ShaderStages::VERTEX,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(size_of::<[[f32; 4]; 3]>() as u64),
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 5,
                visibility: ShaderStages::VERTEX,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(size_of::<[[f32; 4]; 3]>() as u64),
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 6,
                visibility: ShaderStages::VERTEX | ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 7,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Sampler(SamplerBindingType::Filtering),
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 8,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(16),
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 12,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 13,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Sampler(SamplerBindingType::Filtering),
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 14,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            // Player skin arrays of the 64, 128 and 256 texel classes.
            BindGroupLayoutEntry {
                binding: 9,
                visibility: ShaderStages::VERTEX | ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 10,
                visibility: ShaderStages::VERTEX | ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 11,
                visibility: ShaderStages::VERTEX | ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
        ],
    )
}

pub(super) fn actor_pipeline_descriptor(
    bind_group_layout: BindGroupLayoutDescriptor,
) -> RenderPipelineDescriptor {
    let compact = !bind_group_layout
        .entries
        .iter()
        .any(|entry| entry.binding == 5);
    let mut descriptor = RenderPipelineDescriptor {
        label: Some("bounded shared actor pipeline".into()),
        layout: vec![bind_group_layout, crate::lighting::layout()],
        vertex: VertexState {
            shader: ACTOR_SHADER_HANDLE,
            entry_point: Some("actor_vertex".into()),
            buffers: vec![],
            ..default()
        },
        fragment: Some(FragmentState {
            shader: ACTOR_SHADER_HANDLE,
            entry_point: Some("actor_fragment".into()),
            targets: vec![Some(ColorTargetState {
                format: render_scene::SCENE_COLOR_FORMAT,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
            ..default()
        }),
        depth_stencil: Some(DepthStencilState {
            format: CORE_3D_DEPTH_FORMAT,
            depth_write_enabled: Some(true),
            depth_compare: Some(CompareFunction::GreaterEqual),
            stencil: default(),
            bias: default(),
        }),
        ..default()
    };
    if compact {
        descriptor.vertex.shader_defs.push("COMPACT_BONES".into());
        descriptor
            .fragment
            .as_mut()
            .unwrap()
            .shader_defs
            .push("COMPACT_BONES".into());
    }
    descriptor
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct ActorPipelineKey {
    pub(super) msaa: Msaa,
    pub(super) hdr: bool,
    pub(super) enhanced: bool,
    pub(super) material: u32,
    /// Complete raster state; stencil and depth bias do not distinguish pipelines.
    pub(super) raster: render_api::render_state::RenderState,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct ActorPipelineContract {
    msaa: Msaa,
    format: TextureFormat,
    kind: assets::EntityRenderMaterial,
    raster: render_api::render_state::RenderState,
    alpha_to_coverage: bool,
    write_alpha: bool,
}

impl ActorPipelineKey {
    /// The raster state this key draws with.
    pub(super) fn raster(&self) -> render_api::render_state::RenderState {
        crate::actor::material::lowered(self.raster)
    }

    /// Coalesces shader-only material flags while retaining distinct coverage and depth states.
    pub(super) fn contract(self) -> ActorPipelineContract {
        let raster = self.raster();
        let state = crate::actor::material::state(self.material).unwrap_or_default();
        let kind = match self.material & assets::EntityRenderMaterialState::KIND_MASK {
            value if value == assets::EntityRenderMaterial::DissolveDepth as u32 => {
                assets::EntityRenderMaterial::DissolveDepth
            }
            value if value == assets::EntityRenderMaterial::DissolveColor as u32 => {
                assets::EntityRenderMaterial::DissolveColor
            }
            _ => assets::EntityRenderMaterial::Default,
        };
        let format = if self.hdr {
            render_scene::SCENE_HDR_FORMAT
        } else if super::phase::sorted(self.material, &raster)
            && crate::chunk::transparent::gamma_pass::admitted(self.hdr, self.msaa, self.enhanced)
        {
            render_scene::SCENE_COLOR_FORMAT.remove_srgb_suffix()
        } else {
            render_scene::SCENE_COLOR_FORMAT
        };
        ActorPipelineContract {
            msaa: self.msaa,
            format,
            kind,
            raster,
            write_alpha: self.material
                & (render_model::LEGACY_COMBAT_MATERIAL_FLAG
                    | render_model::ACTOR_COLOR_MASK_MATERIAL_FLAG)
                != (render_model::LEGACY_COMBAT_MATERIAL_FLAG
                    | render_model::ACTOR_COLOR_MASK_MATERIAL_FLAG),
            alpha_to_coverage: self.msaa.samples() > 1
                && raster.blend.is_none()
                && (raster.alpha_to_coverage || state.alpha_test)
                && !state.emissive
                && self.material & render_model::ACTOR_COLOR_MASK_MATERIAL_FLAG == 0
                && self.material & assets::EntityRenderMaterialState::KIND_MASK
                    != assets::EntityRenderMaterial::Dragon as u32
                && kind == assets::EntityRenderMaterial::Default,
        }
    }
}

impl SpecializerKey for ActorPipelineKey {
    const IS_CANONICAL: bool = false;
    type Canonical = ActorPipelineContract;
}

impl Specializer<RenderPipeline> for ActorPipelineSpecializer {
    type Key = ActorPipelineKey;

    fn specialize(
        &self,
        key: Self::Key,
        descriptor: &mut RenderPipelineDescriptor,
    ) -> Result<Canonical<Self::Key>, BevyError> {
        let contract = key.contract();
        descriptor.multisample.count = contract.msaa.samples();
        crate::alpha_coverage::apply(descriptor, contract.alpha_to_coverage);
        crate::actor::material::lower_raster(descriptor, &contract.raster);
        let kind = key.material & assets::EntityRenderMaterialState::KIND_MASK;
        if !contract.write_alpha {
            descriptor.fragment.as_mut().unwrap().targets[0]
                .as_mut()
                .unwrap()
                .write_mask &= ColorWrites::COLOR;
        }
        if kind == assets::EntityRenderMaterial::DissolveDepth as u32 {
            descriptor.fragment.as_mut().unwrap().targets[0]
                .as_mut()
                .unwrap()
                .write_mask = ColorWrites::empty();
        } else if kind == assets::EntityRenderMaterial::DissolveColor as u32
            && !matches!(
                contract.raster.depth.compare,
                None | Some(render_api::render_state::CompareFunction::Always)
            )
        {
            // An authored pass-through depth test still wins over the dissolve colour default.
            descriptor.depth_stencil.as_mut().unwrap().depth_compare = Some(CompareFunction::Equal);
        }
        let fragment = descriptor.fragment.as_mut().unwrap();
        fragment.targets[0].as_mut().unwrap().format = contract.format;
        // Sorted-pass spans share the transparent pass's gamma-encoded target.
        if super::phase::sorted(key.material, &contract.raster)
            && crate::chunk::transparent::gamma_pass::admitted(key.hdr, key.msaa, key.enhanced)
        {
            fragment.shader_defs.push(bevy::shader::ShaderDefVal::Bool(
                "ACTOR_GAMMA_BLEND".into(),
                true,
            ));
        }
        Ok(contract)
    }
}
