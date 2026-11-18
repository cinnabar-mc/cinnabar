//! Clears culling decisions and draw routing for failed terrain publications.

use bevy::prelude::World;
use bevy::render::renderer::{RenderDevice, RenderQueue};

use super::{
    GpuCullFrame,
    direct::{DirectOcclusion, DirectOcclusionFrame},
    feedback::GpuCullFeedback,
    prepare::GpuCull,
};

/// Forgets every culling decision so republished allocations rebuild their records.
pub(in crate::chunk) fn invalidate_publication(world: &mut World) {
    let device = world.resource::<RenderDevice>().clone();
    let queue = world.resource::<RenderQueue>().clone();
    if let Some(mut cull) = world.get_resource_mut::<GpuCull>() {
        cull.invalidate_publication(&device, &queue);
    }
    if let Some(mut feedback) = world.get_resource_mut::<GpuCullFeedback>() {
        feedback.select(None, None);
    }
    if let Some(mut direct) = world.get_resource_mut::<DirectOcclusion>() {
        direct.invalidate_publication();
    }
    suppress_publication(world);
}

/// Removes draw routing for a terrain frame whose publication failed.
pub(in crate::chunk) fn suppress_publication(world: &mut World) {
    if let Some(mut cull) = world.get_resource_mut::<GpuCull>() {
        cull.prepared_view = None;
    }
    if let Some(mut frame) = world.get_resource_mut::<GpuCullFrame>() {
        frame.view = None;
    }
    if let Some(mut direct) = world.get_resource_mut::<DirectOcclusion>() {
        direct.suppress_publication();
    }
    if let Some(mut frame) = world.get_resource_mut::<DirectOcclusionFrame>() {
        frame.clear();
    }
}
