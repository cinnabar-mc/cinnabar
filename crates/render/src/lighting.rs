use crate::queue_writes::ProfiledQueue as _;
use bevy::prelude::IntoScheduleConfigs;
use bevy::{
    asset::{load_internal_asset, uuid_handle},
    ecs::{
        query::ROQueryItem,
        system::{SystemParamItem, lifetimeless::SRes},
    },
    prelude::{App, Commands, Handle, Res, ResMut, Resource, Shader},
    render::{
        Render, RenderApp, RenderSystems,
        extract_resource::ExtractResourcePlugin,
        render_phase::{PhaseItem, RenderCommand, RenderCommandResult, TrackedRenderPass},
        render_resource::{
            BindGroup, BindGroupEntry, BindGroupLayoutDescriptor, BindGroupLayoutEntry,
            BindingType, Buffer, BufferBindingType, BufferInitDescriptor, BufferSize, BufferUsages,
            PipelineCache, ShaderStages,
        },
        renderer::{RenderDevice, RenderQueue},
    },
};
use render_model::{LightmapInputs, LightmapUniform};
use render_scene::{WorldFullbright, WorldLighting, WorldPointLight, WorldPointLightSource};

#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
struct PointLightUniform {
    enabled: [f32; 4],
    color: [f32; 4],
    sources: [[f32; 4]; 8],
    strengths: [[f32; 4]; 2],
}

impl From<WorldPointLight> for PointLightUniform {
    fn from(light: WorldPointLight) -> Self {
        let sources = [
            light.local,
            light.additional[0],
            light.additional[1],
            light.additional[2],
            light.additional[3],
            light.additional[4],
            light.additional[5],
            light.additional[6],
        ]
        .map(|source| source_uniform(source, light.enabled));
        Self {
            enabled: [if light.enabled { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0],
            color: [
                finite_channel(light.color[0]),
                finite_channel(light.color[1]),
                finite_channel(light.color[2]),
                0.0,
            ],
            sources: std::array::from_fn(|index| sources[index].0),
            strengths: std::array::from_fn(|group| {
                std::array::from_fn(|channel| sources[group * 4 + channel].1)
            }),
        }
    }
}

fn source_uniform(source: WorldPointLightSource, enabled: bool) -> ([f32; 4], f32) {
    let radius = finite_nonnegative(source.radius);
    let strength = finite_channel(source.intensity);
    let active = enabled && radius > 0.0 && strength > 0.0;
    (
        [
            finite_position(source.position[0]),
            finite_position(source.position[1]),
            finite_position(source.position[2]),
            if active { radius } else { 0.0 },
        ],
        if active { strength } else { 0.0 },
    )
}

fn finite_nonnegative(value: f32) -> f32 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

fn finite_channel(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn finite_position(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

#[derive(Resource)]
pub(crate) struct LightmapGpu {
    buffer: Buffer,
    pub(crate) bind_group: BindGroup,
    inputs: Option<LightmapInputs>,
    atmosphere: Buffer,
    point_light: Buffer,
    point_light_input: PointLightUniform,
}

/// Describes group one shared by every ordinary lit world pipeline.
pub(crate) fn layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "world RGB lightmap",
        &[
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::VERTEX_FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(std::mem::size_of::<LightmapUniform>() as u64),
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(std::mem::size_of::<
                        render_scene::AtmosphereFrame,
                    >() as u64),
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 2,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(
                        std::mem::size_of::<PointLightUniform>() as u64
                    ),
                },
                count: None,
            },
        ],
    )
}

#[derive(Resource)]
struct LightingInstalled;

/// Installs one lightmap upload, independent of terrain or actor publication revisions.
pub(crate) fn install(app: &mut App) {
    app.init_resource::<WorldLighting>()
        .init_resource::<WorldFullbright>()
        .init_resource::<WorldPointLight>();
    let Some(render_app) = app.get_sub_app(RenderApp) else {
        return;
    };
    if render_app.world().contains_resource::<LightingInstalled>() {
        return;
    }
    app.sub_app_mut(RenderApp)
        .insert_resource(LightingInstalled);
    app.add_plugins(ExtractResourcePlugin::<WorldLighting>::default());
    app.add_plugins(ExtractResourcePlugin::<WorldFullbright>::default());
    app.add_plugins(ExtractResourcePlugin::<WorldPointLight>::default());
    const MATERIAL_SHADER: Handle<Shader> = uuid_handle!("40309d5a-76a4-4e3b-aed0-d5c76aa5d52e");

    const SHADER: Handle<Shader> = uuid_handle!("4562a3ce-92ab-46f2-823f-af9faf2cc5c8");
    load_internal_asset!(app, SHADER, "lighting.wesl", |source, path| {
        crate::shader_safety::from_wesl(crate::material_shader::source(source), path)
    });
    load_internal_asset!(app, MATERIAL_SHADER, "material.wesl", |source, path| {
        crate::shader_safety::from_wesl(crate::material_shader::source(source), path)
    });
    const GLINT_SHADER: Handle<Shader> = uuid_handle!("37986856-18a5-4a74-a721-30d08bf0b1d6");
    load_internal_asset!(app, GLINT_SHADER, "glint.wesl", |source, path| {
        crate::shader_safety::from_wesl(crate::material_shader::source(source), path)
    });
    // Terrain, models, liquids and block overlays share one projection.
    const WORLD_PROJECTION_SHADER: Handle<Shader> =
        uuid_handle!("b6411dfb-6a07-4283-ab58-3aedb7cdb856");
    load_internal_asset!(
        app,
        WORLD_PROJECTION_SHADER,
        "world_projection.wesl",
        crate::shader_safety::from_wesl
    );
    app.sub_app_mut(RenderApp)
        .add_systems(Render, prepare.in_set(RenderSystems::PrepareResources));
}

/// Rebuilds the small table only when an environment input changes.
fn prepare(
    mut commands: Commands,
    (input, fullbright, point_light): (
        Res<WorldLighting>,
        Res<WorldFullbright>,
        Res<WorldPointLight>,
    ),
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    cache: Res<PipelineCache>,
    gpu: Option<ResMut<LightmapGpu>>,
    atmosphere: Option<Res<crate::atmosphere_render::AtmosphereGpu>>,
) {
    let inputs = world_lightmap_inputs(&input, &fullbright);
    if let Some(mut gpu) = gpu {
        if gpu.inputs != inputs {
            queue.profiled_write_buffer(
                &gpu.buffer,
                0,
                bytemuck::bytes_of(&LightmapUniform::new(light_table(inputs))),
            );
            gpu.inputs = inputs;
        }
        let mut bind_group_changed = false;
        if let Some(atmosphere) = atmosphere.as_deref()
            && gpu.atmosphere.id() != atmosphere.buffer.id()
        {
            gpu.atmosphere = atmosphere.buffer.clone();
            bind_group_changed = true;
        }
        let point_light_uniform = PointLightUniform::from(*point_light);
        if gpu.point_light_input != point_light_uniform {
            queue.profiled_write_buffer(
                &gpu.point_light,
                0,
                bytemuck::bytes_of(&point_light_uniform),
            );
            gpu.point_light_input = point_light_uniform;
        }
        if bind_group_changed {
            gpu.bind_group = bind_group(
                &device,
                &cache,
                &gpu.buffer,
                &gpu.atmosphere,
                &gpu.point_light,
            );
        }
        return;
    }
    let buffer = device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("world RGB lightmap"),
        contents: bytemuck::bytes_of(&LightmapUniform::new(light_table(inputs))),
        usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
    });
    let atmosphere = atmosphere.map(|gpu| gpu.buffer.clone()).unwrap_or_else(|| {
        device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("fallback world atmosphere"),
            contents: bytemuck::bytes_of(&render_scene::AtmosphereFrame::default()),
            usage: BufferUsages::UNIFORM,
        })
    });
    let point_light_uniform = PointLightUniform::from(*point_light);
    let point_light = device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("world point light"),
        contents: bytemuck::bytes_of(&point_light_uniform),
        usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
    });
    let bind_group = bind_group(&device, &cache, &buffer, &atmosphere, &point_light);
    commands.insert_resource(LightmapGpu {
        buffer,
        bind_group,
        inputs,
        atmosphere,
        point_light,
        point_light_input: point_light_uniform,
    });
}

/// The lightmap inputs ordinary world passes light with this frame; `None` while a personal
/// mod forces full brightness.
pub(crate) fn world_lightmap_inputs(
    lighting: &WorldLighting,
    fullbright: &WorldFullbright,
) -> Option<LightmapInputs> {
    (!fullbright.0).then_some(lighting.0)
}

/// Every sky/block pair's light colour, indexed `sky << 4 | block`; all white under full
/// brightness.
pub(crate) fn light_table(inputs: Option<LightmapInputs>) -> [[f32; 4]; 256] {
    inputs.map_or([[1.0; 4]; 256], LightmapInputs::build)
}

/// Binds the shared environment buffers for ordinary world passes.
fn bind_group(
    device: &RenderDevice,
    cache: &PipelineCache,
    light: &Buffer,
    atmosphere: &Buffer,
    point_light: &Buffer,
) -> BindGroup {
    device.create_bind_group(
        "world lighting and fog",
        &cache.get_bind_group_layout(&layout()),
        &[
            BindGroupEntry {
                binding: 0,
                resource: light.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 1,
                resource: atmosphere.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 2,
                resource: point_light.as_entire_binding(),
            },
        ],
    )
}

pub(crate) struct SetWorldLightmap;
impl<P: PhaseItem> RenderCommand<P> for SetWorldLightmap {
    type Param = SRes<LightmapGpu>;
    type ViewQuery = ();
    type ItemQuery = ();

    fn render<'w>(
        _: &P,
        _: ROQueryItem<'w, '_, Self::ViewQuery>,
        _: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        gpu: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        pass.set_bind_group(1, &gpu.into_inner().bind_group, &[]);
        RenderCommandResult::Success
    }
}

#[cfg(test)]
mod fullbright_tests {
    use super::*;

    #[test]
    fn fullbright_lights_every_sample_and_disabling_restores_current_environment() {
        let dark = LightmapInputs {
            sky_darken: 0.0,
            darkness: 1.0,
            ..Default::default()
        };
        let ordinary = dark.build();
        assert!(ordinary[0][0] < 1.0);
        assert_eq!(light_table(None), [[1.0; 4]; 256]);
        assert_eq!(light_table(Some(dark)), ordinary);
        let day = LightmapInputs {
            brightness: 1.0,
            ..Default::default()
        };
        assert_eq!(light_table(Some(day)), day.build());
    }

    #[test]
    fn point_light_uniform_sanitizes_config_and_keeps_zero_radius_off() {
        let uniform = PointLightUniform::from(WorldPointLight {
            enabled: true,
            color: [2.0, f32::NAN, 0.25],
            local: WorldPointLightSource {
                position: [f32::INFINITY, -64.0, 2.0],
                intensity: 1.0,
                radius: f32::NAN,
            },
            additional: [
                WorldPointLightSource {
                    position: [1.0, 2.0, 3.0],
                    intensity: 2.0,
                    radius: 9.0,
                },
                WorldPointLightSource {
                    position: [4.0, 5.0, 6.0],
                    intensity: f32::NAN,
                    radius: 7.0,
                },
                WorldPointLightSource::default(),
                WorldPointLightSource::default(),
                WorldPointLightSource::default(),
                WorldPointLightSource::default(),
                WorldPointLightSource::default(),
            ],
        });
        assert_eq!(uniform.enabled, [1.0, 0.0, 0.0, 0.0]);
        assert_eq!(uniform.color, [1.0, 0.0, 0.25, 0.0]);
        assert_eq!(
            uniform.sources,
            [
                [0.0, -64.0, 2.0, 0.0],
                [1.0, 2.0, 3.0, 9.0],
                [4.0, 5.0, 6.0, 0.0],
                [0.0; 4],
                [0.0; 4],
                [0.0; 4],
                [0.0; 4],
                [0.0; 4],
            ]
        );
        assert_eq!(uniform.strengths[0], [0.0, 1.0, 0.0, 0.0]);

        let active = PointLightUniform::from(WorldPointLight::default());
        assert_eq!(active.enabled[0], 0.0);
        assert_eq!(active.sources, [[0.0; 4]; 8]);

        let remote_only = PointLightUniform::from(WorldPointLight {
            enabled: true,
            additional: [
                WorldPointLightSource {
                    position: [1.0, 2.0, 3.0],
                    intensity: 0.5,
                    radius: 5.0,
                },
                WorldPointLightSource::default(),
                WorldPointLightSource::default(),
                WorldPointLightSource::default(),
                WorldPointLightSource::default(),
                WorldPointLightSource::default(),
                WorldPointLightSource::default(),
            ],
            ..WorldPointLight::default()
        });
        assert_eq!(remote_only.sources[1], [1.0, 2.0, 3.0, 5.0]);
        assert_eq!(remote_only.strengths[0][1], 0.5);
        assert_eq!(remote_only.sources.len(), 8);
    }
}
