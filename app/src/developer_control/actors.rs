//! Bounded inspection of the last published actor frame, evaluated only on a state request.

use std::collections::BTreeSet;

use bevy::prelude::*;
use render_scene::ActorRenderFrame;
use serde_json::{Value, json};

use {
    crate::{block_entities::SpawnerDisplayPublisher, runtime::world::ClientWorld},
    client_presentation::camera::FlyCamera,
};

const MAX_INSTANCES: usize = 512;
const MAX_MATRIX_ROWS: usize = 8_192;
const MAX_GEOMETRY_VERTICES: usize = 4_096;
const MAX_TOTAL_VERTICES: usize = 16_384;

/// Counts published item instances independently of the player's skin arms.
fn hand_item_counts(instances: &[render_model::ActorGpuInstance]) -> [usize; 2] {
    let mut counts = [0; 2];
    for instance in instances {
        if instance.texture_layer & render_model::HAND_ITEM_LAYER_FLAG != 0 {
            let offhand = instance.texture_layer & render_model::HAND_OFFHAND_LAYER_FLAG != 0;
            counts[usize::from(offhand)] += 1;
        }
    }
    counts
}

/// Reports the last published first-person items for capture acceptance checks.
pub(super) fn hand_snapshot(world: &World) -> Value {
    let Some(frame) = world
        .get_resource::<render_scene::HandRigScene>()
        .and_then(|scene| scene.frame.as_ref())
    else {
        return Value::Null;
    };
    let [main, offhand] = hand_item_counts(&frame.rig.instances);
    json!({
        "frame_generation": frame.rig.frame_generation,
        "geometry_revision": frame.rig.geometry_revision,
        "main_item_instances": main,
        "offhand_item_instances": offhand,
    })
}

fn camera(world: &World) -> Option<Value> {
    world.archetypes().iter().flat_map(|archetype| archetype.entities()).find_map(|entity| {
        let entity = world.get_entity(entity.id()).ok()?;
        entity.get::<FlyCamera>()?;
        let transform = entity.get::<Transform>()?;
        let projection = entity.get::<Projection>()?;
        let clip_from_view = projection.get_clip_from_view();
        let global = entity.get::<GlobalTransform>().map(GlobalTransform::to_matrix);
        Some(json!({
            "position": transform.translation.to_array(),
            "rotation_xyzw": transform.rotation.to_array(),
            "world_from_view": transform.to_matrix().to_cols_array_2d(),
            "clip_from_world": (clip_from_view * transform.to_matrix().inverse()).to_cols_array_2d(),
            "global_clip_from_world": global.map(|matrix| (clip_from_view * matrix.inverse()).to_cols_array_2d()),
        }))
    })
}

fn matrices(arena: &[[[f32; 4]; 3]], base: u32, count: usize) -> Option<&[[[f32; 4]; 3]]> {
    let start = base as usize;
    arena.get(start..start.checked_add(count)?)
}

/// Resolves a published lifetime within its own namespace; `true` marks a client-owned display.
pub(crate) fn manifest_actor(
    world: &World,
    session_id: u64,
    runtime_id: u64,
) -> Option<(&client_world::ActorSnapshot, bool)> {
    let network = world
        .get_resource::<ClientWorld>()
        .and_then(|world| world.stream.as_ref())
        .map(|stream| stream.authority())
        .filter(|authority| authority.actor_session_id() == session_id);
    if let Some(authority) = network {
        return authority.actor(runtime_id).map(|actor| (actor, false));
    }
    world
        .get_resource::<SpawnerDisplayPublisher>()?
        .actor(session_id, runtime_id)
        .map(|actor| (actor, true))
}

pub(super) fn snapshot(world: &World) -> Value {
    let Some(frame) = world.get_resource::<ActorRenderFrame>() else {
        return json!({ "available": false, "camera": camera(world) });
    };
    let rig = &frame.rig;
    let mut entries = rig
        .manifest
        .iter()
        .filter(|entry| {
            render_model::is_pack_rig_id(entry.rig)
                || manifest_actor(world, entry.identity.session_id, entry.identity.runtime_id)
                    .is_some_and(|(actor, _)| {
                        matches!(actor.kind, protocol::ActorKind::Entity { .. })
                    })
        })
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| {
        (
            !render_model::is_pack_rig_id(entry.rig),
            entry.identity.runtime_id,
            entry.identity.layer,
        )
    });
    let total = entries.len();
    let mut matrix_budget = MAX_MATRIX_ROWS;
    let mut vertex_budget = MAX_TOTAL_VERTICES;
    let mut geometries = Vec::new();
    let mut seen_geometry = BTreeSet::new();
    let instances = entries
        .into_iter()
        .take(MAX_INSTANCES)
        .map(|entry| {
            let identity = entry.identity;
            let Some(instance) = rig.instances.get(entry.instance_index as usize) else {
                return json!({ "runtime_id": identity.runtime_id, "invalid_instance_index": entry.instance_index });
            };
            let resolved = manifest_actor(world, identity.session_id, identity.runtime_id);
            let actor = resolved.map(|(actor, _)| actor);
            let identifier = actor.and_then(|actor| match &actor.kind {
                protocol::ActorKind::Entity { identifier } => Some(identifier.as_ref()),
                protocol::ActorKind::Player { .. } => None,
            });
            let count = (entry.bone_count as usize)
                .min(render_model::MAX_RENDER_BONES_PER_ACTOR)
                .min(matrix_budget / 2);
            matrix_budget -= count * 2;
            let page = frame.instance_pages().get(entry.instance_index as usize).copied();
            let texture = page
                .and_then(|page| usize::from(page).checked_sub(1))
                .and_then(|index| frame.artwork_pages().pages().get(index));
            if seen_geometry.insert(instance.geometry_id) {
                let span = rig.geometry_spans.get(instance.geometry_id as usize);
                let vertices = span.and_then(|span| rig.geometry_vertices.span(*span));
                let listed = if render_model::is_pack_rig_id(entry.rig) {
                    vertices.map_or(0, |vertices| vertices.len())
                        .min(MAX_GEOMETRY_VERTICES)
                        .min(vertex_budget)
                } else {
                    0
                };
                vertex_budget -= listed;
                let samples = vertices.map(|vertices| {
                    vertices.iter().take(listed).map(|vertex| json!({
                        "position": vertex.position,
                        "normal": vertex.normal,
                        "uv": vertex.uv,
                        "back_uv": vertex.back_uv,
                        "bone_index": vertex.bone_index,
                        "opposing_faces": vertex.surface == render_model::ActorRigSurface::OPPOSING_FACES,
                    })).collect::<Vec<_>>()
                });
                geometries.push(json!({
                    "geometry_id": instance.geometry_id,
                    "rig": entry.rig.0,
                    "first_vertex": span.map(|span| span.first_vertex),
                    "vertex_count": span.map(|span| span.vertex_count),
                    "listed_vertices": listed,
                    "vertices": samples,
                }));
            }
            json!({
                "session_id": identity.session_id,
                "dimension": identity.dimension,
                "runtime_id": identity.runtime_id,
                "unique_id": actor.map(|actor| actor.unique_id),
                "identifier": identifier,
                "display_actor": resolved.map(|(_, display)| display),
                "actor_lifetime_matches": actor.map(|actor| actor.render_spawn_revision == identity.spawn_revision),
                "spawn_revision": identity.spawn_revision,
                "ingress_sequence": identity.ingress_sequence,
                "source_tick": identity.source_tick,
                "movement_revision": identity.movement_revision,
                "pose_generation": identity.pose_generation.get(),
                "layer": identity.layer,
                "rig": entry.rig.0,
                "route": format!("{:?}", entry.route),
                "completed_tick": entry.completed_tick,
                "reset_generation": entry.reset_generation,
                "instance_index": entry.instance_index,
                "material_word": instance.material,
                "material_kind": instance.material & assets::EntityRenderMaterialState::KIND_MASK,
                "material_state": assets::EntityRenderMaterialState::from_word(instance.material),
                "dissolve_multiplier": instance.dissolve_multiplier,
                "light_color_multiplier": instance.light_color_multiplier,
                "geometry_id": instance.geometry_id,
                "texture_page": page,
                "texture_layer": instance.texture_layer,
                "texture_dimensions": texture.map(|texture| texture.dimensions()),
                "texture_page_layers": texture.map(|texture| texture.layers()),
                "multitexture_layers": instance.multitexture_layers,
                "light": instance.light,
                "tint": instance.tint,
                "overlay_rgba8": instance.overlay_rgba8,
                "uv_anim": instance.uv_anim,
                "partial_tick": instance.partial_tick,
                "world_from_actor": instance.world_from_actor,
                "previous_bone_base": instance.previous_bone_base,
                "current_bone_base": instance.current_bone_base,
                "bone_count": entry.bone_count,
                "listed_bones": count,
                "previous_bones": matrices(&rig.previous_bones, instance.previous_bone_base, count),
                "current_bones": matrices(&rig.current_bones, instance.current_bone_base, count),
            })
        })
        .collect::<Vec<_>>();
    json!({
        "available": true,
        "frame_generation": rig.frame_generation,
        "geometry_revision": rig.geometry_revision,
        "instance_revision": frame.instance_revision,
        "skin_revision": frame.skin_revision,
        "artwork_identity": frame.artwork_pages().identity(),
        "total_frame_instances": rig.instances.len(),
        "generic_instances": total,
        "listed_instances": instances.len(),
        "omitted_instances": total.saturating_sub(instances.len()),
        "matrix_rows": MAX_MATRIX_ROWS - matrix_budget,
        "geometry_vertices": MAX_TOTAL_VERTICES - vertex_budget,
        "camera": camera(world),
        "instances": instances,
        "geometries": geometries,
    })
}

#[cfg(test)]
mod hand_tests {
    use super::*;

    #[test]
    fn hand_draw_counts_items_without_counting_skin_arms() {
        let item = render_model::HAND_ITEM_LAYER_FLAG;
        let offhand = render_model::HAND_OFFHAND_LAYER_FLAG;
        let instances = [0, item, item | 1, item | offhand, offhand].map(|texture_layer| {
            render_model::ActorGpuInstance {
                texture_layer,
                ..Default::default()
            }
        });
        assert_eq!(hand_item_counts(&instances), [2, 1]);
        assert_eq!(hand_item_counts(&[]), [0, 0]);
        assert!(hand_snapshot(&World::new()).is_null());
    }
}
