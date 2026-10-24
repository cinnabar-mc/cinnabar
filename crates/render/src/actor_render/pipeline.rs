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
    /// Unique built-in draw and motion pipelines for each view, retained for readiness and
    /// shader reloads.
    prewarmed: std::collections::HashMap<(Msaa, bool, bool), Vec<CachedRenderPipelineId>>,
    /// Pipelines the current session's authored materials and frames requested, per view.
    /// A pack change retires them from readiness; the pipelines themselves stay in Bevy's
    /// cache, which cannot destroy them, and are reused if a later pack requests them again.
    session_prewarmed: std::collections::HashMap<(Msaa, bool, bool), Vec<CachedRenderPipelineId>>,
    /// The session's published authored rasters and the views that already requested them.
    session_rasters: (AuthoredRasters, Vec<(Msaa, bool, bool)>),
    /// (view, word, raster) pairs a frame drew that are already requested, so an unchanged
    /// pair costs one lookup on later frames.
    frame_requests: std::collections::HashSet<(
        (Msaa, bool, bool),
        u32,
        render_api::render_state::RenderState,
    )>,
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
            session_prewarmed: Default::default(),
            session_rasters: (std::sync::Arc::from([]), Vec::new()),
            frame_requests: Default::default(),
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
            let ids = self.session_prewarmed.entry(view).or_default();
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
                let ids = self.session_prewarmed.entry(view).or_default();
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
        Some(())
    }

    /// Requests the (word, raster) pairs a frame's instances draw with that this view has not
    /// requested yet. An instance whose raster index is outside the table draws unauthored,
    /// which prewarming already covers.
    pub(super) fn prewarm_frame(
        &mut self,
        cache: &PipelineCache,
        view: (Msaa, bool, bool),
        rig: &render_model::ActorRigRenderFrame,
    ) -> Option<()> {
        for instance in rig.instances.iter() {
            let Some(&raster) = rig.rasters.get(instance.raster as usize) else {
                continue;
            };
            let request = (view, instance.material, raster);
            if self.frame_requests.contains(&request) {
                continue;
            }
            self.prewarm_rasters(cache, view, [(instance.material, raster)])?;
            self.frame_requests.insert(request);
        }
        Some(())
    }

    /// Adopts a session's published authored rasters. Equal contents keep the views that
    /// already requested them, so an unchanged reload requests nothing; a change retires the
    /// previous session's pipelines from readiness.
    pub(super) fn adopt_session_rasters(&mut self, rasters: &AuthoredRasters) {
        if std::sync::Arc::ptr_eq(&self.session_rasters.0, rasters) {
            return;
        }
        if *self.session_rasters.0 == **rasters {
            self.session_rasters.0 = std::sync::Arc::clone(rasters);
            return;
        }
        self.session_rasters = (std::sync::Arc::clone(rasters), Vec::new());
        self.session_prewarmed.clear();
        self.frame_requests.clear();
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
            && self
                .session_prewarmed
                .get(&view)
                .is_none_or(|ids| ids.iter().copied().all(&mut compiled))
    }
}

#[cfg(test)]
#[path = "pipeline_readiness_tests.rs"]
mod readiness_tests;

/// The (instance word, raster) pairs the generated native material table, and a layer that
/// names no material, draw with under every presentation variant: both combat profiles, glint,
/// dye masks, overlay suppression and the 1.12 worn material. Raster states a server pack
/// authors arrive separately as the session's rasters, so only this table compiles before any
/// pack.
fn prewarm_materials() -> impl Iterator<Item = (u32, render_api::render_state::RenderState)> {
    use assets::facts::pack::MaterialKind;
    let rules = assets::facts::pack::rules();
    let unauthored = render_model::ActorMaterial::default();
    let native = rules
        .generated_material_states()
        .map(move |(name, native)| {
            let class = rules.material(name);
            render_model::ActorMaterial {
                kind: rules.raster(name),
                state: Some(assets::EntityRenderMaterialState {
                    // Blended native materials draw in the transparent pass without a cutout.
                    alpha_test: native.alpha_test && !native.blend,
                    emissive: native.emissive,
                    only_emissive: native.only_emissive,
                    disable_overlay: native.disable_overlay,
                    color_mask: class == Some(MaterialKind::ColorMask),
                    multitexture: class == Some(MaterialKind::Multitexture),
                    // The native table names no glint, tint or mask defines.
                    ..Default::default()
                }),
                render_state: Some(native.definition().render_state()),
                ..Default::default()
            }
        });
    std::iter::once(unauthored)
        .chain(native)
        .flat_map(render_model::ActorMaterial::presentation_rasters)
        .flat_map(with_late_dissolve)
}

/// A (word, raster) pair, plus the late pass the renderer splits a dissolve colour draw into.
fn with_late_dissolve(
    (word, raster): (u32, render_api::render_state::RenderState),
) -> impl Iterator<Item = (u32, render_api::render_state::RenderState)> {
    let late = (word & assets::EntityRenderMaterialState::KIND_MASK
        == assets::EntityRenderMaterial::DissolveColor as u32)
        .then_some(word | crate::actor::material::LATE_DISSOLVE_COLOR);
    std::iter::once(word)
        .chain(late)
        .map(move |word| (word, raster))
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
    if let Some(frame) = &frame {
        pipeline.adopt_session_rasters(&frame.authored_rasters);
    }
    let changed = frame.filter(bevy::ecs::change_detection::DetectChanges::is_changed);
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
                    .prewarm_rasters(
                        &cache,
                        view,
                        session.iter().copied().flat_map(with_late_dissolve),
                    )
                    .is_some()
                {
                    pipeline.session_rasters.1.push(view);
                }
            }
            ready &= pipeline
                .prewarm(&cache, msaa, camera.hdr, enhanced.is_some())
                .is_some()
                && changed
                    .as_ref()
                    .is_none_or(|frame| pipeline.prewarm_frame(&cache, view, &frame.rig).is_some())
                && pipeline.ready(&cache, msaa, camera.hdr, enhanced.is_some());
        }
    }
    readiness.publish(has_view && ready);
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
    /// The raster state as the pipeline observes it: see [`ActorPipelineKey::contract`].
    raster: render_api::render_state::RenderState,
    alpha_to_coverage: bool,
}

impl ActorPipelineKey {
    /// The raster state this key draws with.
    pub(super) fn raster(&self) -> render_api::render_state::RenderState {
        crate::actor::material::lowered(self.raster)
    }

    /// Coalesces shader-only material flags while retaining distinct coverage and depth states.
    /// Raster fields a pipeline cannot observe collapse, so draws that compile to the same
    /// pipeline share one contract: a dissolve depth pass writes no colour, so its blend and
    /// write mask do not matter; a dissolve colour pass with a depth test always tests for
    /// equality; nothing blends into a target without writes; and an untested draw writes no
    /// depth.
    pub(super) fn contract(self) -> ActorPipelineContract {
        use render_api::render_state::CompareFunction as Compare;
        let mut raster = self.raster();
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
        let alpha_to_coverage = self.msaa.samples() > 1
            && raster.blend.is_none()
            && (raster.alpha_to_coverage || state.alpha_test)
            && !state.emissive
            && self.material & render_model::ACTOR_COLOR_MASK_MATERIAL_FLAG == 0
            && self.material & assets::EntityRenderMaterialState::KIND_MASK
                != assets::EntityRenderMaterial::Dragon as u32
            && kind == assets::EntityRenderMaterial::Default;
        // Legacy combat dye keeps the destination alpha the dye mask reads.
        let dyed = render_model::LEGACY_COMBAT_MATERIAL_FLAG
            | render_model::ACTOR_COLOR_MASK_MATERIAL_FLAG;
        if self.material & dyed == dyed {
            raster.write_alpha = false;
        }
        match kind {
            assets::EntityRenderMaterial::DissolveDepth => {
                raster.write_color = false;
                raster.write_alpha = false;
            }
            assets::EntityRenderMaterial::DissolveColor
                if !matches!(raster.depth.compare, None | Some(Compare::Always)) =>
            {
                raster.depth.compare = Some(Compare::Equal);
            }
            _ => {}
        }
        if !raster.write_color && !raster.write_alpha {
            raster.blend = None;
        }
        if raster.depth.compare.is_none() {
            raster.depth.compare = Some(Compare::Always);
            raster.depth.write = false;
        }
        // Coverage is decided above; the shared raster field is not lowered on its own.
        raster.alpha_to_coverage = false;
        ActorPipelineContract {
            msaa: self.msaa,
            format,
            kind,
            raster,
            alpha_to_coverage,
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
        let fragment = descriptor.fragment.as_mut().unwrap();
        fragment.targets[0].as_mut().unwrap().format = contract.format;
        // Sorted-pass spans share the transparent pass's gamma-encoded target.
        if super::phase::sorted(key.material, &key.raster())
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
