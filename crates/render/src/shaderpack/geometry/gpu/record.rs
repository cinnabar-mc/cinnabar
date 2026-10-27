//! Recording the frame's terrain expansion dispatches.

use super::*;

/// Records this frame's expansion dispatches ahead of camera passes.
pub(crate) fn record_geometry_expansion(
    geometry: Option<ResMut<ShaderpackGeometry>>,
    pipelines: Option<Res<ExpansionPipelines>>,
    kernels: Option<Res<crate::shaderpack::kernels::Kernels>>,
    timestamps: Option<Res<crate::gpu_timing::GpuTimestamps>>,
    mut context: RenderContext,
) {
    let (Some(mut geometry), Some(pipelines), Some(kernels)) = (geometry, pipelines, kernels)
    else {
        return;
    };
    let cache = &kernels.cache;
    let prepared = std::mem::take(&mut geometry.prepared);
    if prepared.dispatches.is_empty() {
        return;
    }
    crate::gpu_timing::within_stage(
        timestamps.as_deref(),
        RuntimeStage::GpuShaderpackExpansion,
        context.command_encoder(),
        |encoder| {
            let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
                label: Some("shader-pack terrain expansion"),
                ..default()
            });
            for dispatch in &prepared.dispatches {
                let Some(pipeline) =
                    cache.get_compute_pipeline(pipelines.pipelines[dispatch.kind as usize])
                else {
                    continue;
                };
                let groups = dispatch.jobs.div_ceil(EXPAND_WORKGROUP_SIZE);
                if groups == 0 {
                    continue;
                }
                let x = groups.min(u16::MAX as u32);
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, &dispatch.sources, &[]);
                pass.set_bind_group(1, &dispatch.output, &[]);
                pass.dispatch_workgroups(x, groups.div_ceil(x), 1);
            }
        },
    );
}
