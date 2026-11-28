//! On-demand inspection of the final CPU-published first-person frame.
use render_scene::hand_rig::HandRigFrame;
use serde_json::{Value, json};

/// Counts items by their hand role, independently of skin and catalog models.
pub(super) fn item_counts(instances: &[render_model::ActorGpuInstance]) -> [usize; 2] {
    let mut counts = [0; 2];
    for instance in instances {
        if instance.texture_layer & render_model::HAND_ITEM_LAYER_FLAG != 0 {
            let offhand = instance.texture_layer & render_model::HAND_OFFHAND_LAYER_FLAG != 0;
            counts[usize::from(offhand)] += 1;
        }
    }
    counts
}

const MAX_DRAWS: usize = 512;
const MAX_BONE_MATRICES: usize = 8_192;
const MAX_VERTICES: usize = 16_384;

/// Reads immutable publication data only when the control endpoint requests state.
pub(super) fn snapshot(frame: &HandRigFrame) -> Value {
    let rig = &frame.rig;
    let [main, offhand] = item_counts(&rig.instances);
    let mut bone_budget = MAX_BONE_MATRICES;
    let mut serialized_matrices = 0;
    let mut vertex_budget = MAX_VERTICES;
    let mut seen = std::collections::BTreeSet::new();
    let mut geometries = Vec::new();
    let draws = rig.instances.iter().enumerate().take(MAX_DRAWS).map(|(index, instance)| {
        let entry = rig.manifest.iter().find(|entry| entry.instance_index as usize == index);
        let full_count = entry.map_or(0, |entry| entry.bone_count as usize);
        let count = full_count.min(bone_budget / 2);
        bone_budget -= count * 2;
        let previous = render_api::bone_range(&rig.previous_bones, instance.previous_bone_base, count, full_count);
        let current = render_api::bone_range(&rig.current_bones, instance.current_bone_base, count, full_count);
        let previous_count = previous.matrices.map_or(0, |bones| bones.len());
        let current_count = current.matrices.map_or(0, |bones| bones.len());
        let listed_bones = previous_count.min(current_count);
        serialized_matrices += previous_count + current_count;
        let location = frame.draw_locations.get(index).copied().flatten();
        let page = location.and_then(|location| usize::from(location.page).checked_sub(1))
            .and_then(|index| frame.artwork.as_ref()?.pages().get(index));
        if seen.insert(instance.geometry_id) {
            let span = rig.geometry_spans.get(instance.geometry_id as usize);
            let vertices = span.and_then(|span| rig.geometry_vertices.span(*span));
            let listed = vertices.map_or(0, |vertices| vertices.len()).min(vertex_budget);
            vertex_budget -= listed;
            geometries.push(json!({
                "geometry_id": instance.geometry_id,
                "first_vertex": span.map(|span| span.first_vertex),
                "vertex_count": span.map(|span| span.vertex_count),
                "listed_vertices": listed,
                "omitted_vertices": span.map(|span| (span.vertex_count as usize).saturating_sub(listed)),
                "vertices": vertices.map(|vertices| vertices.iter().take(listed).map(|vertex| json!({
                    "position": vertex.position, "normal": vertex.normal, "uv": vertex.uv,
                    "bone_index": vertex.bone_index,
                })).collect::<Vec<_>>()),
            }));
        }
        json!({
            "instance_index": index,
            "rig": entry.map(|entry| entry.rig.0),
            "identity": entry.map(|entry| format!("{:?}", entry.identity)),
            "route": entry.map(|entry| format!("{:?}", entry.route)),
            "completed_tick": entry.map(|entry| entry.completed_tick),
            "geometry_id": instance.geometry_id,
            "texture_layer": instance.texture_layer,
            "artwork_location": location.map(|location| json!({
                "page": location.page, "layer": location.layer,
                "pose_mode": format!("{:?}", location.pose_mode), "multitexture": location.multitexture,
                "dimensions": page.map(|page| page.dimensions()),
                "page_layers": page.map(|page| page.layers()),
                "valid_layer": page.is_some_and(|page| location.layer < page.layers()),
            })),
            "location_entry_present": index < frame.draw_locations.len(),
            "material_word": instance.material,
            "raster_index": instance.raster,
            "raster_state": rig.rasters.get(instance.raster as usize).map(|state| format!("{state:?}")),
            "tint": instance.tint, "overlay_rgba8": instance.overlay_rgba8,
            "uv_anim": instance.uv_anim, "multitexture_layers": instance.multitexture_layers,
            "dissolve_multiplier": instance.dissolve_multiplier,
            "light_color_multiplier": instance.light_color_multiplier,
            "world_from_actor": instance.world_from_actor,
            "partial_tick": instance.partial_tick,
            "reset_generation": instance.reset_generation,
            "previous_bone_base": instance.previous_bone_base,
            "current_bone_base": instance.current_bone_base,
            "bone_count": entry.map(|entry| entry.bone_count),
            "listed_bones": listed_bones,
            "omitted_bones": entry.map(|entry| (entry.bone_count as usize).saturating_sub(listed_bones)),
            "previous_bone_range_valid": previous.full_range_valid,
            "current_bone_range_valid": current.full_range_valid,
            "previous_bones": previous.matrices,
            "current_bones": current.matrices,
        })
    }).collect::<Vec<_>>();
    json!({
        "frame_generation": rig.frame_generation,
        "geometry_revision": rig.geometry_revision,
        "revision": frame.revision, "fov_radians": frame.fov_radians,
        "light": format!("{:?}", frame.light),
        "main_item_instances": main, "offhand_item_instances": offhand,
        "total_instances": rig.instances.len(), "listed_instances": draws.len(),
        "omitted_instances": rig.instances.len().saturating_sub(draws.len()),
        "draw_location_count": frame.draw_locations.len(),
        "maximum_vertex_count": rig.maximum_vertex_count,
        "rejects": format!("{:?}", rig.rejects),
        "artwork_identity": frame.artwork.as_ref().map(|pages| pages.identity()),
        "artwork_page_count": frame.artwork.as_ref().map(|pages| pages.pages().len()),
        "item_atlases": frame.item_atlases.each_ref().map(|atlas| atlas.as_ref().map(|atlas| json!({
            "width": atlas.width, "height": atlas.height, "layers": atlas.layers,
        }))),
        "bone_matrices": serialized_matrices,
        "bone_matrix_budget_used": MAX_BONE_MATRICES - bone_budget,
        "geometry_vertices": MAX_VERTICES - vertex_budget,
        "draws": draws, "geometries": geometries,
    })
}

#[cfg(test)]
#[path = "hand_frame_tests.rs"]
mod tests;
