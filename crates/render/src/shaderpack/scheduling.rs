//! Schedules pack work only while existing load or render owners still need it.

use super::{
    atlas::AtlasBuild,
    geometry::{ShaderpackGeometry, ShaderpackGeometrySettings},
    kernels::Kernels,
    load_failures::ViewFailures,
    plugin::{LoadedShaderPack, Loader},
};
use bevy::{
    app::SubApp,
    prelude::*,
    render::{Extract, MainWorld, Render, sync_world::RenderEntity},
};
use render_scene::shaderpack::{ShaderPackRendering, ShaderPackRequest};

/// Optional render preparation shares one lifecycle check per render frame.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct ShaderPackSystems;

/// Whether a load, publication or cancellation still has a main-world owner.
fn main_owners_pending(world: &World) -> bool {
    world
        .get_resource::<ShaderPackRequest>()
        .is_some_and(|request| request.pack.is_some())
        || world
            .get_resource::<Loader>()
            .is_some_and(|loader| loader.key.is_some() || loader.job.is_some())
        || world
            .get_resource::<LoadedShaderPack>()
            .is_some_and(|pack| pack.0.is_some())
        || world
            .get_resource::<ViewFailures>()
            .is_some_and(ViewFailures::is_pending)
}

/// Keeps load owners and explicitly opted-in cameras active until they leave.
pub(super) fn main_work_pending(
    world: &World,
    cameras: Query<(), With<ShaderPackRendering>>,
) -> bool {
    main_owners_pending(world) || !cameras.is_empty()
}

/// Extracts while a main-world load or an existing render owner still needs shader inputs.
pub(super) fn extraction_pending(
    world: &World,
    cameras: Extract<Query<(), With<ShaderPackRendering>>>,
) -> bool {
    world
        .get_resource::<MainWorld>()
        .is_some_and(|main| main_owners_pending(main))
        || !cameras.is_empty()
        || render_work_pending(world)
}

/// Copies opted-in cameras; the sync plugin removes their markers when main cameras leave.
pub(super) fn extract_cameras(
    mut commands: Commands,
    cameras: Extract<Query<&RenderEntity, With<ShaderPackRendering>>>,
) {
    commands.try_insert_batch(
        cameras
            .iter()
            .map(|entity| (entity.id(), ShaderPackRendering))
            .collect::<Vec<_>>(),
    );
}

/// Runs the unload frame and continues only while an atlas worker is retiring.
pub(super) fn render_work_pending(world: &World) -> bool {
    world
        .get_resource_ref::<LoadedShaderPack>()
        .is_some_and(|pack| pack.0.is_some() || (pack.is_changed() && !pack.is_added()))
        || world
            .get_resource::<ShaderpackGeometrySettings>()
            .is_some_and(|settings| settings.enabled)
        || world.contains_resource::<ShaderpackGeometry>()
        || world.contains_resource::<Kernels>()
        || world
            .get_resource::<AtlasBuild>()
            .is_some_and(AtlasBuild::has_work)
}

/// Keeps opted-in cameras compatible with the host's single-sample depth reads.
pub(super) fn admit_cameras(
    mut cameras: Query<(&mut bevy::render::view::Msaa, &mut Camera3d), With<ShaderPackRendering>>,
) {
    for (mut msaa, mut camera) in &mut cameras {
        msaa.set_if_neq(bevy::render::view::Msaa::Off);
        let usage = wgpu::TextureUsages::from(camera.depth_texture_usages)
            | wgpu::TextureUsages::TEXTURE_BINDING;
        if wgpu::TextureUsages::from(camera.depth_texture_usages) != usage {
            camera.depth_texture_usages = usage.into();
        }
    }
}

/// Gates extraction and render preparation with their existing load and render owners.
pub(super) fn configure_render(app: &mut SubApp) {
    app.configure_sets(Render, ShaderPackSystems.run_if(render_work_pending))
        .configure_sets(
            bevy::render::ExtractSchedule,
            ShaderPackSystems.run_if(extraction_pending),
        );
}

#[cfg(test)]
mod tests;
