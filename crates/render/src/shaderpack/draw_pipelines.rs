//! Builds geometry draw pipelines from translated programs and per-draw render state.

use super::alpha_test::inject_alpha_test;
use super::draws::DrawKind;
use super::effects::EffectSettings;
use super::geometry::{TerrainLayout, vertex_buffer_layouts};
use super::pipelines::{blend_state, color_target};
use shaderpack::{AlphaTest, BufferId, ProgramReflection, ShaderProperties, StageKind};
use std::borrow::Cow;
use wgpu::TextureFormat;

/// What building one draw's pipeline needs from its program.
pub(crate) struct DrawPipelineInputs<'a> {
    pub device: &'a wgpu::Device,
    pub cache: Option<&'a wgpu::PipelineCache>,
    pub layout: &'a wgpu::PipelineLayout,
    /// The resolved program's name, which `alphaTest.*` and `blend.*` rules name.
    pub name: &'a str,
    pub reflection: &'a ProgramReflection,
    /// The program's vertex stage module.
    pub vertex: &'a wgpu::ShaderModule,
    pub emulated: bool,
    /// The program's translated fragment stage, which the alpha test is injected into.
    pub fragment: &'a naga::Module,
    pub properties: &'a ShaderProperties,
    /// The pack's effect directives (`rain.depth`).
    pub effects: &'a EffectSettings,
    /// Format of the target an output's draw buffer names (`colortexN`, or `shadowcolorN`
    /// for shadow draws); `None` drops the output.
    pub target_format: &'a dyn Fn(u32) -> Option<TextureFormat>,
    /// The expanded vertex terrain draws read; other draws read [`TerrainLayout::WIDE`].
    pub terrain_layout: TerrainLayout,
}

/// The draw buffer a `blend.<program>.<buffer>` rule names, in the draw's buffer space.
fn rule_buffer(name: &str, shadow: bool) -> Option<u32> {
    match (BufferId::parse(name)?, shadow) {
        (BufferId::Color(i), false) | (BufferId::ShadowColor(i), true) => Some(i),
        _ => None,
    }
}

/// The vertex layout `kind` reads: the pack's terrain layout for terrain, the wide stream the
/// host builds for everything else.
fn draw_layout(kind: DrawKind, terrain: TerrainLayout) -> TerrainLayout {
    if kind.category().is_some() {
        terrain
    } else {
        TerrainLayout::WIDE
    }
}

/// The pipeline, colour targets and alpha test built for one geometry draw.
pub(crate) struct BuiltDrawPipeline {
    pub pipeline: wgpu::RenderPipeline,
    pub outputs: Vec<Option<u32>>,
    pub alpha_test: Option<AlphaTest>,
    /// The depth-only pipeline drawn before this one; with it, this pipeline tests the
    /// prepass depth without writing depth.
    pub prepass: Option<wgpu::RenderPipeline>,
    /// For the sky: the same pipeline testing (never writing) the terrain prepass depth.
    pub behind_terrain: Option<wgpu::RenderPipeline>,
}

/// Builds the pipeline of `kind` with one program: the geometry vertex layout, Iris's
/// culling and depth state (geometry winds counter-clockwise; the translator's clip-space
/// Y flip mirrors it to clockwise), the alpha test and the blend state per draw buffer.
pub(crate) fn draw_pipeline(
    kind: DrawKind,
    inputs: &DrawPipelineInputs<'_>,
) -> Result<BuiltDrawPipeline, String> {
    let reflection = inputs.reflection;
    let layouts = if kind.actor() {
        super::actors::layout::vertex_layouts(&reflection.vertex_inputs)
    } else {
        vertex_buffer_layouts(
            &reflection.vertex_inputs,
            &draw_layout(kind, inputs.terrain_layout),
        )
    }
    .map_err(|e| e.to_string())?;
    let buffers: Vec<_> = layouts
        .iter()
        .map(|layout| {
            Some(wgpu::VertexBufferLayout {
                array_stride: layout.array_stride,
                step_mode: layout.step_mode,
                attributes: &layout.attributes,
            })
        })
        .collect();
    let rules = &inputs.properties.blends;
    let program_blend = rules
        .iter()
        .rev()
        .find(|b| b.program == inputs.name && b.buffer.is_none())
        .map(|b| b.mode);
    let len = reflection
        .color_outputs
        .iter()
        .map(|o| o.location as usize + 1)
        .max()
        .unwrap_or(0);
    let mut targets = vec![None; len];
    let mut outputs = vec![None; len];
    for output in &reflection.color_outputs {
        let Some(buffer) = output.buffer else {
            continue;
        };
        let Some(format) = (inputs.target_format)(buffer) else {
            continue;
        };
        let buffer_blend = rules
            .iter()
            .rev()
            .find(|r| {
                r.program == inputs.name
                    && r.buffer
                        .as_deref()
                        .and_then(|b| rule_buffer(b, kind.shadow()))
                        == Some(buffer)
            })
            .map(|r| r.mode);
        let mode = buffer_blend
            .or(program_blend)
            .unwrap_or(kind.default_blend());
        let target = color_target(format, output.ty, blend_state(mode));
        if target.is_some() {
            outputs[output.location as usize] = Some(buffer);
        }
        targets[output.location as usize] = target;
    }
    if !kind.depth() && targets.iter().all(Option::is_none) {
        return Err("it writes no buffer the frame has".into());
    }
    let alpha = kind.alpha_test(inputs.name, &inputs.properties.alpha_tests);
    let tested = alpha.and_then(|test| {
        let mut module = inputs.fragment.clone();
        inject_alpha_test(&mut module, test).then_some(module)
    });
    let fragment_module = tested.unwrap_or_else(|| inputs.fragment.clone());
    let prepass_fragment = (prepass_eligible(kind, inputs)
        && targets
            .iter()
            .flatten()
            .all(|target| target.blend.is_none()))
    .then(|| super::depth_prepass::depth_only_fragment(&fragment_module))
    .flatten();
    let fragment = super::pipelines::pack_shader_module(
        inputs.device,
        wgpu::ShaderModuleDescriptor {
            label: Some(&format!("shader pack {} {kind:?} fsh", inputs.name)),
            source: wgpu::ShaderSource::Naga(Cow::Owned(fragment_module)),
        },
    );
    let entry = |stage: StageKind| {
        reflection
            .entry_points
            .iter()
            .find(|(s, _)| *s == stage)
            .map(|(_, e)| e.as_str())
    };
    let label = format!("shader pack {} {kind:?}", inputs.name);
    let primitive = wgpu::PrimitiveState {
        topology: wgpu::PrimitiveTopology::TriangleList,
        front_face: wgpu::FrontFace::Cw,
        cull_mode: kind.culls().then_some(wgpu::Face::Back),
        ..Default::default()
    };
    let vertex = wgpu::VertexState {
        module: inputs.vertex,
        entry_point: entry(StageKind::Vertex),
        compilation_options: Default::default(),
        buffers: if inputs.emulated { &[] } else { &buffers },
    };
    let prepass = prepass_fragment.map(|module| {
        let module = super::pipelines::pack_shader_module(
            inputs.device,
            wgpu::ShaderModuleDescriptor {
                label: Some(&format!("shader pack {} {kind:?} depth fsh", inputs.name)),
                source: wgpu::ShaderSource::Naga(Cow::Owned(module)),
            },
        );
        inputs
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(&format!("{label} depth prepass")),
                layout: Some(inputs.layout),
                vertex: vertex.clone(),
                primitive,
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: TextureFormat::Depth32Float,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(kind.depth_compare()),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: entry(StageKind::Fragment),
                    compilation_options: Default::default(),
                    targets: &[],
                }),
                multiview_mask: None,
                cache: inputs.cache,
            })
    });
    let behind_terrain = (matches!(kind, DrawKind::Sky | DrawKind::Sunset)
        && !inputs.emulated
        && !super::pipelines::stores(inputs.reflection))
    .then(|| {
        inputs
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(&format!("{label} behind terrain")),
                layout: Some(inputs.layout),
                vertex: vertex.clone(),
                primitive,
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: TextureFormat::Depth32Float,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &fragment,
                    entry_point: entry(StageKind::Fragment),
                    compilation_options: Default::default(),
                    targets: &targets,
                }),
                multiview_mask: None,
                cache: inputs.cache,
            })
    });
    let pipeline = inputs
        .device
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(&label),
            layout: Some(inputs.layout),
            vertex,
            primitive,
            depth_stencil: kind.depth().then_some(wgpu::DepthStencilState {
                format: TextureFormat::Depth32Float,
                depth_write_enabled: Some(kind.depth_write(inputs.effects) && prepass.is_none()),
                depth_compare: Some(kind.depth_compare()),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &fragment,
                entry_point: entry(StageKind::Fragment),
                compilation_options: Default::default(),
                targets: &targets,
            }),
            multiview_mask: None,
            cache: inputs.cache,
        });
    Ok(BuiltDrawPipeline {
        pipeline,
        outputs,
        alpha_test: alpha,
        prepass,
        behind_terrain,
    })
}

/// Opaque terrain whose program has no side effects a second rasterization could repeat.
fn prepass_eligible(kind: DrawKind, inputs: &DrawPipelineInputs<'_>) -> bool {
    matches!(kind, DrawKind::TerrainSolid | DrawKind::TerrainCutout)
        && !inputs.emulated
        && !super::pipelines::stores(inputs.reflection)
}
