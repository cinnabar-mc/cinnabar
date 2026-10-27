use std::collections::{BTreeMap, BTreeSet};

use crate::{EntityRenderData, EntityRenderLayer, EntityRenderMaterial, EntityRenderMaterialState};

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

/// Sources whose fractional alpha reaches the shader: dissolve masks, Cinnabar material masks,
/// art any drawing material blends, lights, colour-masks or multitextures by alpha, and art every
/// drawing material alpha-tests. Every other raster's alpha rounds at 0.5.
pub fn actor_sources_keeping_alpha(render: &EntityRenderData) -> BTreeSet<u32> {
    // Per source: (every drawing material alpha-tests, some drawing material reads alpha).
    let mut drawn = BTreeMap::<u32, (bool, bool)>::new();
    for layer in &render.layers {
        for (_, state, definition) in render.materials(layer) {
            let blends = definition.is_some_and(|definition| {
                definition.has_state(crate::entity::material::EntityMaterialState::Blending)
            });
            let reads = blends
                || state.is_some_and(|state| {
                    state.emissive || state.color_mask || state.samples_extra_textures()
                });
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
    keep.extend(actor_material_mask_sources(render));
    keep
}

/// Mask textures Cinnabar materials sample; their alpha weights the mask.
pub fn actor_material_mask_sources(render: &EntityRenderData) -> impl Iterator<Item = u32> + '_ {
    render
        .material_masks
        .iter()
        .flat_map(|masks| [masks.glow, masks.emissive].into_iter().flatten())
}

/// Sources any possible material of a layer that samples extra textures reads
/// ([`EntityRenderMaterialState::samples_extra_textures`]), including Molang material choices;
/// they share one array page so the draw can read them together.
pub fn actor_multitexture_sources(render: &EntityRenderData) -> ActorMultitextureSources {
    ActorMultitextureSources::from_draws(
        render
            .layers
            .iter()
            .filter(|layer| {
                render.materials(layer).any(|(_, state, _)| {
                    state.is_some_and(EntityRenderMaterialState::samples_extra_textures)
                })
            })
            .map(|layer| layer_sources(render, layer)),
    )
}

/// The sources draws that sample extra textures read, kept per draw so that every source one draw
/// samples can be placed on the same page.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ActorMultitextureSources {
    /// Each draw's distinct sources, ascending.
    draws: Box<[Box<[u32]>]>,
    sources: BTreeSet<u32>,
}

impl ActorMultitextureSources {
    /// Collects the sources of each draw; a draw without sources is dropped.
    pub fn from_draws<D: IntoIterator<Item = u32>>(draws: impl IntoIterator<Item = D>) -> Self {
        let draws: Box<[Box<[u32]>]> = draws
            .into_iter()
            .map(|draw| draw.into_iter().collect::<BTreeSet<_>>())
            .filter(|draw| !draw.is_empty())
            .map(|draw| draw.into_iter().collect())
            .collect();
        let sources = draws.iter().flat_map(|draw| draw.iter().copied()).collect();
        Self { draws, sources }
    }

    /// Whether any draw samples `source`.
    #[must_use]
    pub fn contains(&self, source: &u32) -> bool {
        self.sources.contains(source)
    }

    /// The distinct sources of each draw, ascending.
    pub fn draws(&self) -> impl Iterator<Item = &[u32]> {
        self.draws.iter().map(AsRef::as_ref)
    }
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

/// How a pack's entity sources must share artwork pages so one draw samples them together.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ActorPageSharing {
    /// Sources each draw of a material sampling extra textures reads
    /// ([`actor_multitexture_sources`]); a companion group holding one of them joins as one more
    /// draw, so its masks share a page with the textures they mask.
    pub multitexture: ActorMultitextureSources,
    /// The base textures and masks of layers drawing Cinnabar material masks, each mapped to the
    /// smallest source it must share a page with. Sources absent here share with nothing.
    pub companions: BTreeMap<u32, u32>,
    /// Mask sources, repeated on every page of a companion group too large for one page so each
    /// base keeps its masks.
    pub masks: BTreeSet<u32>,
}

/// The page sharing `render`'s layers need: each layer drawing Cinnabar masks keeps every base
/// texture it can draw with every mask it can select, and multitexture sampling spreads to the
/// masks drawn over those textures.
pub fn actor_page_sharing(render: &EntityRenderData) -> ActorPageSharing {
    let mut parent = BTreeMap::<u32, u32>::new();
    for layer in &render.layers {
        let selected = render.material_nodes(layer).filter_map(|node| match node {
            crate::EntityRenderMaterialSelection::Raster { masks, .. } => *masks,
            _ => None,
        });
        let masks: BTreeSet<u32> = layer
            .material_selection
            .is_none()
            .then_some(layer.material_masks)
            .flatten()
            .into_iter()
            .chain(selected)
            .filter_map(|index| render.material_masks.get(index as usize))
            .flat_map(|masks| [masks.glow, masks.emissive].into_iter().flatten())
            .collect();
        let Some(&first) = masks.first() else {
            continue;
        };
        for source in layer_sources(render, layer).chain(masks.iter().copied()) {
            join(&mut parent, first, source);
        }
    }
    let sources: Vec<u32> = parent.keys().copied().collect();
    let companions: BTreeMap<u32, u32> = sources
        .into_iter()
        .map(|source| (source, root(&mut parent, source)))
        .collect();
    let sampled = actor_multitexture_sources(render);
    let shared: BTreeSet<u32> = companions
        .iter()
        .filter(|(source, _)| sampled.contains(source))
        .map(|(_, root)| *root)
        .collect();
    let groups = shared.iter().map(|group| {
        companions
            .iter()
            .filter(move |(_, root)| *root == group)
            .map(|(source, _)| *source)
            .collect::<Vec<_>>()
    });
    let multitexture =
        ActorMultitextureSources::from_draws(sampled.draws().map(<[u32]>::to_vec).chain(groups));
    ActorPageSharing {
        multitexture,
        companions,
        masks: actor_material_mask_sources(render).collect(),
    }
}

/// The smallest source of `source`'s set, compressing the path to it.
fn root(parent: &mut BTreeMap<u32, u32>, source: u32) -> u32 {
    let mut root = *parent.entry(source).or_insert(source);
    while parent[&root] != root {
        root = parent[&root];
    }
    parent.insert(source, root);
    root
}

/// Merges the sets of `a` and `b` under the smaller of their roots.
fn join(parent: &mut BTreeMap<u32, u32>, a: u32, b: u32) {
    let (a, b) = (root(parent, a), root(parent, b));
    parent.insert(a.max(b), a.min(b));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(layers: serde_json::Value) -> EntityRenderData {
        serde_json::from_value(serde_json::json!({
            "layers": layers,
            "slots": [
                { "first_candidate": 0, "candidate_count": 2 },
                { "first_candidate": 2, "candidate_count": 1 },
                { "first_candidate": 3, "candidate_count": 1 },
            ],
            "candidates": [
                { "condition": null, "source": 4 },
                { "condition": null, "source": 5 },
                { "condition": null, "source": 6 },
                { "condition": null, "source": 7 },
            ],
            "visibility": [],
            "material_masks": [
                { "glow": 2, "emissive": 3, "strength": 1.0 },
                { "glow": 8, "strength": 1.0 },
            ],
        }))
        .unwrap()
    }

    fn layer(
        first_slot: u32,
        slot_count: u16,
        masks: Option<u32>,
        multitexture: bool,
    ) -> serde_json::Value {
        serde_json::json!({
            "material_masks": masks,
            "material_state": multitexture.then(|| serde_json::json!({
                "alpha_test": false,
                "multitexture": true,
            })),
            "rig": 0,
            "condition": null,
            "first_slot": first_slot,
            "slot_count": slot_count,
            "first_visibility": 0,
            "visibility_count": 0,
            "color": null,
            "overlay_color": null,
            "on_fire_color": null,
        })
    }

    #[test]
    fn mask_layers_keep_every_base_with_every_mask() {
        let sharing = actor_page_sharing(&render(serde_json::json!([
            layer(0, 1, Some(0), false),
            layer(2, 1, None, false),
        ])));
        assert_eq!(
            sharing.companions,
            BTreeMap::from([(2, 2), (3, 2), (4, 2), (5, 2)]),
            "both base candidates share with both masks; the unmasked layer shares with nothing"
        );
        assert!(sharing.multitexture.draws().next().is_none());
        assert_eq!(sharing.masks, BTreeSet::from([2, 3, 8]));
    }

    #[test]
    fn masks_over_multitexture_sources_share_their_pages() {
        let sharing = actor_page_sharing(&render(serde_json::json!([
            layer(1, 2, Some(1), true),
            layer(0, 1, Some(0), false),
        ])));
        assert_eq!(
            sharing
                .multitexture
                .draws()
                .flatten()
                .copied()
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([6, 7, 8])
        );
        assert_eq!(sharing.companions[&8], 6);
        assert_eq!(sharing.companions[&4], 2);
    }
}
