use std::collections::{BTreeMap, BTreeSet};

use crate::{EntityRenderData, EntityRenderLayer, EntityRenderMaterial};

/// Dissolve depth masks retain fractional alpha because their threshold changes each frame.
fn actor_dissolve_mask_sources(render: &EntityRenderData) -> BTreeSet<u32> {
    render
        .layers
        .iter()
        .filter(|layer| {
            render
                .materials(layer)
                .any(|(material, ..)| material == EntityRenderMaterial::DissolveDepth)
        })
        .flat_map(|layer| layer_sources(render, layer))
        .collect()
}

/// Sources whose fractional alpha reaches the shader: dissolve masks, art any drawing material
/// blends, lights, colour-masks or multitextures by alpha, and art every drawing material
/// alpha-tests. Every other raster's alpha rounds at 0.5.
pub fn actor_sources_keeping_alpha(render: &EntityRenderData) -> BTreeSet<u32> {
    // Per source: (every drawing material alpha-tests, some drawing material reads alpha).
    let mut drawn = BTreeMap::<u32, (bool, bool)>::new();
    for layer in &render.layers {
        for (_, state, definition) in render.materials(layer) {
            let blends = definition.is_some_and(|definition| {
                definition.has_state(crate::entity::material::EntityMaterialState::Blending)
            });
            let reads = blends
                || state
                    .is_some_and(|state| state.emissive || state.color_mask || state.multitexture);
            for source in layer_sources(render, layer) {
                let (tests, read) = drawn.entry(source).or_insert((true, false));
                *tests &= state.is_some_and(|state| state.alpha_test);
                *read |= reads;
            }
        }
    }
    let mut keep = actor_dissolve_mask_sources(render);
    keep.extend(
        drawn
            .into_iter()
            .filter(|(_, (tests, reads))| *tests || *reads)
            .map(|(source, _)| source),
    );
    keep
}

/// Sources a `USE_MULTITEXTURE` material samples; they share one array page so the draw can
/// read all three.
pub fn actor_multitexture_sources(render: &EntityRenderData) -> BTreeSet<u32> {
    render
        .layers
        .iter()
        .filter(|layer| layer.material_state.is_some_and(|state| state.multitexture))
        .flat_map(|layer| layer_sources(render, layer))
        .collect()
}

fn layer_sources<'a>(
    render: &'a EntityRenderData,
    layer: &'a EntityRenderLayer,
) -> impl Iterator<Item = u32> + 'a {
    render
        .slots
        .get(layer.first_slot as usize..)
        .unwrap_or_default()
        .iter()
        .take(usize::from(layer.slot_count))
        .flat_map(|slot| {
            render
                .candidates
                .get(slot.first_candidate as usize..)
                .unwrap_or_default()
                .iter()
                .take(usize::from(slot.candidate_count))
        })
        .map(|candidate| candidate.source)
}
