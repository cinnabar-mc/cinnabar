//! Schedules bounded block-grid work using immutable session assets and registries.

use bevy::{
    prelude::*,
    tasks::{AsyncComputeTaskPool, Task, block_on, poll_once},
};
use mod_render::retained::{DrawError, ErrorCode};
use std::sync::Arc;

/// At most one block generation runs at a time, including across owner reloads.
#[derive(Resource, Default)]
pub(crate) struct BlockJobs(Option<Task<()>>);

/// Publishes worker results through the retained generation; idle frames only check readiness.
pub(crate) fn prepare(
    scene: Res<render_scene::ModRenderScene>,
    world: Res<crate::runtime::world::ClientWorld>,
    registries: Option<Res<client_interaction::movement::PhysicsCollisionRegistries>>,
    mut jobs: Local<BlockJobs>,
) {
    let busy = jobs
        .0
        .as_mut()
        .is_some_and(|task| block_on(poll_once(task)).is_none());
    if !busy {
        jobs.0 = None;
    }
    let Some(stream) = world
        .stream
        .as_ref()
        .filter(|_| !world.dimension_transfer.active())
    else {
        return;
    };
    let Some(registries) = registries else {
        return;
    };
    let authority = stream.authority();
    for resources in &scene.retained {
        for request in resources
            .geometry
            .values()
            .filter_map(|geometry| geometry.blocks())
        {
            if !request.bind_context(
                authority.runtime_assets(),
                authority.network_id_mode(),
                stream.mesh_render_settings(),
            ) {
                request.fail_render(DrawError {
                    code: ErrorCode::Unsupported,
                    detail: "block registry or meshing settings changed",
                });
            }
        }
    }
    if busy {
        return;
    }
    for resources in &scene.retained {
        if resources.connection != authority.actor_session_id()
            || resources.dimension != authority.current_dimension()
        {
            continue;
        }
        let used_bytes: usize = resources
            .geometry
            .values()
            .filter_map(|geometry| geometry.blocks()?.result()?.as_ref().ok())
            .map(|mesh| mesh.bytes)
            .sum();
        let used_triangles: usize = resources
            .geometry
            .values()
            .filter_map(|geometry| geometry.blocks()?.result()?.as_ref().ok())
            .map(|mesh| mesh.triangles)
            .sum();
        for geometry in resources.geometry.values() {
            let Some(request) = geometry.blocks() else {
                continue;
            };
            if !request.bind_context(
                authority.runtime_assets(),
                authority.network_id_mode(),
                stream.mesh_render_settings(),
            ) {
                request.fail_render(DrawError {
                    code: ErrorCode::Unsupported,
                    detail: "block registry or meshing settings changed",
                });
                continue;
            }
            if request.render_error().is_some() || !request.claim() {
                continue;
            }
            let request = Arc::clone(request);
            let assets = Arc::clone(authority.runtime_assets());
            let registries = (**registries).clone();
            let mode = authority.network_id_mode();
            let settings = stream.mesh_render_settings();
            jobs.0 = Some(AsyncComputeTaskPool::get().spawn(async move {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    request.mesh(
                        &assets,
                        mode,
                        |name, states| {
                            let states = serde_json::from_str(states).ok()?;
                            registries.block_state_runtime_id(
                                mode,
                                assets::facts::BlockStateIdentity::Named(name),
                                &states,
                            )
                        },
                        settings,
                    )
                }))
                .unwrap_or(Err(DrawError {
                    code: ErrorCode::Unsupported,
                    detail: "block meshing worker",
                }));
                let result = result.and_then(|mesh| {
                    if used_bytes.saturating_add(mesh.bytes)
                        > mod_api::drawing::MAX_BLOCK_MESH_BYTES
                        || used_triangles.saturating_add(mesh.triangles)
                            > mod_api::drawing::MAX_BLOCK_TRIANGLES
                    {
                        Err(DrawError {
                            code: ErrorCode::BudgetExceeded,
                            detail: "owner block meshes",
                        })
                    } else {
                        Ok(mesh)
                    }
                });
                request.complete(result);
            }));
            return;
        }
    }
}
