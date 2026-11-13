//! Upload staging released under host memory pressure; later missed frames grow the pools again.

use super::{BufferUploadStaging, TerrainUploadStaging};
use crate::device_poll::FramesInFlight;
use bevy::{
    prelude::*,
    render::{Extract, renderer::RenderDevice},
};
use memory_pressure::MemoryPressure;
use std::time::Duration;

/// How long the frame that leaves the screen waits for submitted frames to finish. No later
/// frame polls the device, so anything those frames retire would otherwise stay allocated for
/// as long as the client is hidden.
const BACKGROUND_COMPLETION_WAIT: Duration = Duration::from_millis(250);

/// The strongest pressure extracted from the main world and not yet answered here.
#[derive(Resource, Default)]
pub(super) struct PendingRelease(Option<MemoryPressure>);

/// Carries this frame's pressure to the render world.
pub(super) fn extract_memory_pressure(
    mut pressure: Extract<MessageReader<MemoryPressure>>,
    mut pending: ResMut<PendingRelease>,
) {
    if let Some(strongest) = pressure.read().copied().max() {
        pending.0 = pending.0.max(Some(strongest));
    }
}

/// Drops the idle staging slots the pools grew into. A client leaving the screen first waits,
/// within [`BACKGROUND_COMPLETION_WAIT`], for its submitted frames, so the slots they used and
/// the buffers and textures they retired are released before rendering stops.
pub(super) fn release_staging_growth(
    mut pending: ResMut<PendingRelease>,
    device: Res<RenderDevice>,
    mut frames: ResMut<FramesInFlight>,
    frame: Option<Res<BufferUploadStaging>>,
    terrain: Option<Res<TerrainUploadStaging>>,
) {
    let Some(pressure) = pending.bypass_change_detection().0.take() else {
        return;
    };
    if pressure == MemoryPressure::Background
        && let Err(error) =
            frames.finish_outstanding(device.wgpu_device(), BACKGROUND_COMPLETION_WAIT)
    {
        debug!(
            ?error,
            "frames were still on the GPU when the client left the screen"
        );
    }
    let slots: usize = [
        frame.as_deref().map(|staging| &staging.0),
        terrain.as_deref().map(|staging| &staging.0),
    ]
    .into_iter()
    .flatten()
    .map(|staging| staging.release_growth())
    .sum();
    debug!(?pressure, slots, "released upload staging growth");
}

#[cfg(test)]
mod tests {
    use super::super::pool::TERRAIN;
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    /// A render world holding both production pools on the validation-only device.
    fn world() -> World {
        let (device, _queue) = wgpu::Device::noop(&Default::default());
        let device = RenderDevice::from(device);
        let mut world = World::new();
        world.insert_resource(BufferUploadStaging::new(&device));
        world.insert_resource(TerrainUploadStaging::new(&device));
        world.insert_resource(device);
        world.init_resource::<FramesInFlight>();
        world.init_resource::<PendingRelease>();
        world
    }

    /// The terrain pool's capacity after one real growth step.
    fn grow_terrain(world: &World) -> u64 {
        let terrain = world.resource::<TerrainUploadStaging>();
        terrain.request_test_growth();
        terrain.lock().grow();
        terrain.capacity_bytes()
    }

    #[test]
    fn pressure_returns_grown_pools_to_their_initial_capacity() {
        for pressure in [
            MemoryPressure::Low,
            MemoryPressure::Critical,
            MemoryPressure::Background,
        ] {
            let mut world = world();
            let initial = world.resource::<TerrainUploadStaging>().capacity_bytes();
            assert_eq!(grow_terrain(&world), initial + TERRAIN.slot_bytes);

            world.run_system_once(release_staging_growth).unwrap();
            assert_eq!(
                world.resource::<TerrainUploadStaging>().capacity_bytes(),
                initial + TERRAIN.slot_bytes,
                "nothing is released without pressure"
            );

            world.resource_mut::<PendingRelease>().0 = Some(pressure);
            world.run_system_once(release_staging_growth).unwrap();
            let terrain = world.resource::<TerrainUploadStaging>();
            assert_eq!(terrain.capacity_bytes(), initial, "{pressure:?}");
            assert!(world.resource::<PendingRelease>().0.is_none());

            // The pool is not capped at its floor afterwards: the next misses grow it again.
            assert_eq!(grow_terrain(&world), initial + TERRAIN.slot_bytes);
        }
    }
}
