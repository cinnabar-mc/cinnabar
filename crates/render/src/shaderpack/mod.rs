//! Experimental "Shader pack" render mode: runs a Java OptiFine/Iris shader pack, translated
//! by the `shaderpack` crate, on Cinnabar's world. Loading and GPU pipeline creation wait
//! until the player selects a pack.
//!
//! Flow: the app writes `ShaderPackRequest`; a worker thread translates the pack and builds
//! raw wgpu pipelines from the naga modules (see `pipelines`, `draws`), publishing
//! `ShaderPackStatus` and the loaded pack. Cameras marked `ShaderPackRendering` then run the
//! frame as Iris does (see `passes`): the `shadow*` programs over resident terrain, actors
//! and block entities into the shadow maps, `shadowcomp*` compute passes, `prepare*`,
//! `gbuffers_skybasic` over the sky discs, opaque terrain with the `gbuffers_terrain*`
//! programs, actors (`gbuffers_entities`, `spidereyes`, `armor_glint`), block entities
//! (`gbuffers_block`) and the hand (`gbuffers_hand`), `deferred*`, translucent terrain,
//! actors, block entities and hand, `composite*` and `final`, before tonemapping and the UI.
//! Terrain comes from the geometry adapter (`geometry`) and the block atlas (`atlas`);
//! actors, block entities and the hand from the actor adapter (`actors`). The pack also
//! draws the sun, moon, stars, particles, clouds, weather, lightning, block cracks and the
//! selection outline through its own programs (`effects`). What it does not draw (block-entity
//! portals and unsupported actor materials) draws natively after final, against restored
//! pack depth. Native hands keep their own depth and screen overlays draw before the HUD. The
//! Vanilla terrain and far-terrain passes skip the pack's cameras (`ShaderPackDrawnCategories`
//! `terrain`), so a section not expanded yet shows the pack's sky or fog, as Iris draws no
//! unbuilt section. Packs without terrain programs get the Vanilla scene copied into
//! `colortex0` instead.
//!
//! Conventions: pack buffers are stored bottom-up and gamma-encoded as in OpenGL Minecraft
//! (the translator flips clip-space Y and remaps clip depth), uniforms use OpenGL matrices,
//! and each texture is read with the sampler class Iris gives its name.

pub(crate) mod actors;
mod alpha_test;
pub(crate) mod atlas;
mod center_depth;
mod dispatch;
mod draw_pipelines;
mod draws;
pub(crate) mod effects;
mod frame_geometry;
pub(crate) mod geometry;
mod geometry_shader;
#[cfg(test)]
mod geometry_shader_tests;
mod host_pipelines;
mod ids;
mod kernels;
mod lightmap;
mod load_failures;
mod loader;
mod native_passes;
mod overlay_depth;
mod passes;
mod pipeline_cache;
mod pipelines;
mod plan;
pub(crate) mod plugin;
mod resources;
mod scene_bindings;
mod scheduling;
mod shadow_maps;
mod sky;
mod targets;
#[cfg(test)]
mod tests;
mod uniform_audit;
mod uniforms;

pub(crate) mod native_actors;

use bevy::prelude::{App, Plugin};

/// Runs native scene draws at their ordinary boundary or after a ready pack's final pass.
pub(crate) fn native_pass_enabled(
    world: &bevy::prelude::World,
    view: bevy::prelude::Entity,
    after_pack: bool,
) -> bool {
    let replacement = plugin::draws_world(world, view);
    replacement == after_pack
}

/// Loads the requested shader pack and runs it on opted-in cameras.
#[derive(Debug, Clone, Copy, Default)]
pub struct ShaderPackRenderPlugin;

impl Plugin for ShaderPackRenderPlugin {
    fn build(&self, app: &mut App) {
        render_scene::startup_timing::measure("shader_pack_build", || plugin::build(app));
    }

    fn finish(&self, app: &mut App) {
        render_scene::startup_timing::measure("shader_pack_finish", || plugin::finish(app));
    }
}
