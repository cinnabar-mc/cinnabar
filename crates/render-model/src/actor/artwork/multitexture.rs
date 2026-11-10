//! Rasters a `USE_MULTITEXTURE` or `MASKED_MULTITEXTURE` material samples share one array page.
//! Nearest texel replication preserves each source's normalized-UV lookup; RGB composition
//! remains in the fragment shader.

use crate::{ActorArtworkLocation, ActorArtworkPages};

use super::*;

pub(super) fn page_dimensions(catalog: &RuntimeActorCatalog) -> (u16, u16) {
    largest(
        catalog
            .textures()
            .iter()
            .enumerate()
            .filter(|(index, _)| catalog.texture_uses_multitexture(*index))
            .map(|(_, texture)| texture),
    )
}

/// The smallest page that holds every one of `textures` at full resolution.
pub(super) fn largest<'a>(textures: impl Iterator<Item = &'a assets::ActorTexture>) -> (u16, u16) {
    textures.fold((0, 0), |(width, height), texture| {
        (width.max(texture.width), height.max(texture.height))
    })
}

/// Groups the pack textures that `USE_MULTITEXTURE` draws sample into pages. Textures one draw
/// samples are joined, transitively across draws that share a texture; each joined set is sized
/// to its largest texture, and sets of one size share pages up to the layer limit without a
/// set ever spanning two pages. Only a single set past the limit is split.
pub(super) fn pack_pages(
    textures: &[assets::ActorTexture],
    sources: &assets::ActorMultitextureSources,
) -> Vec<((u16, u16), Vec<usize>)> {
    use std::collections::HashMap;
    let mut by_source = HashMap::<u32, Vec<usize>>::new();
    for (index, texture) in textures.iter().enumerate() {
        if sources.contains(&texture.source) {
            by_source.entry(texture.source).or_default().push(index);
        }
    }
    // Union-find over texture indices, rooted at the smallest member.
    let mut parent: Vec<usize> = (0..textures.len()).collect();
    fn root(parent: &mut [usize], mut index: usize) -> usize {
        while parent[index] != index {
            parent[index] = parent[parent[index]];
            index = parent[index];
        }
        index
    }
    for draw in sources.draws() {
        let mut members = draw
            .iter()
            .filter_map(|source| by_source.get(source))
            .flatten()
            .copied();
        let Some(first) = members.next() else {
            continue;
        };
        for member in members {
            let (a, b) = (root(&mut parent, first), root(&mut parent, member));
            parent[a.max(b)] = a.min(b);
        }
    }
    let mut sets = BTreeMap::<usize, Vec<usize>>::new();
    for index in (0..textures.len()).filter(|index| sources.contains(&textures[*index].source)) {
        let set = root(&mut parent, index);
        sets.entry(set).or_default().push(index);
    }
    let mut by_size = BTreeMap::<(u16, u16), Vec<Vec<usize>>>::new();
    for set in sets.into_values() {
        let size = largest(set.iter().map(|index| &textures[*index]));
        by_size.entry(size).or_default().push(set);
    }
    let mut pages = Vec::new();
    for (size, sets) in by_size {
        let mut page = Vec::new();
        for set in sets {
            if !page.is_empty()
                && page.len() + set.len() > super::page_budget::MAX_ACTOR_PAGE_LAYERS
            {
                pages.push((size, std::mem::take(&mut page)));
            }
            page.extend(set);
        }
        if !page.is_empty() {
            pages.push((size, page));
        }
    }
    pages
}

/// Replicates one layer and box-filters it to `limit` without allocating the expanded image.
pub(super) fn replicated(
    texture: &assets::ActorTexture,
    width: u16,
    height: u16,
    limit: u32,
) -> Arc<[u8]> {
    let [out_width, out_height] = crate::fitted_dimensions(width, height, limit);
    if (texture.width, texture.height) == (width, height)
        && (out_width, out_height) == (width, height)
    {
        return Arc::clone(&texture.rgba8);
    }
    let mut pixels = Vec::with_capacity(usize::from(out_width) * usize::from(out_height) * 4);
    append_fitted(&mut pixels, texture, width, height, limit);
    pixels.into()
}

/// Builds catalog layers directly at their budgeted page dimensions.
pub(super) fn page_pixels(
    catalog: &RuntimeActorCatalog,
    indices: &[usize],
    width: u16,
    height: u16,
    limit: u32,
) -> Vec<u8> {
    let [out_width, out_height] = crate::fitted_dimensions(width, height, limit);
    let mut pixels =
        Vec::with_capacity(usize::from(out_width) * usize::from(out_height) * 4 * indices.len());
    for &index in indices {
        let texture = &catalog.textures()[index];
        append_fitted(&mut pixels, texture, width, height, limit);
    }
    pixels
}

/// Counts replicated texels of each source texel within one box-filter interval.
fn replication_weights(
    start: usize,
    end: usize,
    source: usize,
    expanded: usize,
) -> impl Iterator<Item = (usize, u64)> {
    let first = start * source / expanded;
    let last = (end - 1) * source / expanded;
    (first..=last).map(move |texel| {
        let low = (texel * expanded).div_ceil(source).max(start);
        let high = ((texel + 1) * expanded).div_ceil(source).min(end);
        (texel, (high - low) as u64)
    })
}

/// Averages replicated boxes using source-texel weights, without visiting repeated texels.
fn append_fitted(
    pixels: &mut Vec<u8>,
    texture: &assets::ActorTexture,
    width: u16,
    height: u16,
    limit: u32,
) {
    let factor = crate::actor::texel_fit::reduction(width, height, limit);
    if factor == 1 {
        append_nearest(pixels, texture, width, height);
        return;
    }
    let [out_width, out_height] = crate::fitted_dimensions(width, height, limit).map(usize::from);
    let (width, height) = (usize::from(width), usize::from(height));
    let (source_width, source_height) = (usize::from(texture.width), usize::from(texture.height));
    for y in 0..out_height {
        let (top, bottom) = (y * factor, ((y + 1) * factor).min(height));
        for x in 0..out_width {
            let (left, right) = (x * factor, ((x + 1) * factor).min(width));
            let mut sums = [0u64; 4];
            for (sy, rows) in replication_weights(top, bottom, source_height, height) {
                for (sx, columns) in replication_weights(left, right, source_width, width) {
                    let at = (sy * source_width + sx) * 4;
                    for (sum, value) in sums.iter_mut().zip(&texture.rgba8[at..at + 4]) {
                        *sum += u64::from(*value) * rows * columns;
                    }
                }
            }
            let count = ((right - left) * (bottom - top)) as u64;
            pixels.extend(sums.map(|sum| (sum / count) as u8));
        }
    }
}

/// Appends one full-resolution layer with nearest texel replication.
fn append_nearest(pixels: &mut Vec<u8>, texture: &assets::ActorTexture, width: u16, height: u16) {
    if (texture.width, texture.height) == (width, height) {
        pixels.extend_from_slice(&texture.rgba8);
        return;
    }
    // Witnessed native dimensions are integer subdivisions of their shared page. Never filter
    // the tiny alpha masks or interpolate art into a different texture.
    for y in 0..usize::from(height) {
        for x in 0..usize::from(width) {
            let source_x = x * usize::from(texture.width) / usize::from(width);
            let source_y = y * usize::from(texture.height) / usize::from(height);
            let at = (source_y * usize::from(texture.width) + source_x) * 4;
            pixels.extend_from_slice(&texture.rgba8[at..at + 4]);
        }
    }
}

impl ActorArtworkPages {
    /// Routes samplers together; two-texture materials repeat their second source.
    pub fn multitexture_location(
        &self,
        rig: EntityRigId,
        sources: [u32; 3],
    ) -> Option<ActorArtworkLocation> {
        self.combine_samplers(sources, |source| self.variant_location(rig, source))
    }

    /// Routes samplers together for a catalog model without its own artwork route, such as a
    /// player's cosmetic controller, as [`Self::source_location`] routes one texture.
    pub fn source_multitexture_location(
        &self,
        pack: bool,
        sources: [u32; 3],
    ) -> Option<ActorArtworkLocation> {
        self.combine_samplers(sources, |source| self.source_location(pack, source))
    }

    /// Joins the base location with its two extra samplers when they share one texture page.
    fn combine_samplers(
        &self,
        sources: [u32; 3],
        locate: impl Fn(u32) -> Option<ActorArtworkLocation>,
    ) -> Option<ActorArtworkLocation> {
        let mut base = locate(sources[0])?;
        let second = locate(sources[1])?;
        let third = locate(sources[2])?;
        if base.page != second.page || base.page != third.page {
            return None;
        }
        let page = self.pages.get(usize::from(base.page).checked_sub(1)?)?;
        if !page.multitexture && sources[1] != sources[2] {
            return None;
        }
        base.multitexture = Some([second.layer, third.layer]);
        Some(base)
    }

    pub(super) fn valid_multitexture(&self, location: ActorArtworkLocation) -> bool {
        let Some(layers) = location.multitexture else {
            return true;
        };
        let Some(page) = usize::from(location.page)
            .checked_sub(1)
            .and_then(|index| self.pages.get(index))
        else {
            return false;
        };
        (page.multitexture || layers[0] == layers[1])
            && layers.iter().all(|&layer| {
                layer < page.layers
                    && (self.entity_locations.contains(&(location.page, layer))
                        || self.pack_locations.contains(&(location.page, layer)))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ActorTexturePage;

    #[test]
    fn masked_multitexture_routes_two_session_pack_samplers_on_one_page() {
        let rig = crate::pack_rig_id(0);
        let first = ActorArtworkLocation {
            page: 1,
            layer: 0,
            pose_mode: assets::ActorPoseMode::CompiledLiteral,
            multitexture: None,
        };
        let second = ActorArtworkLocation { layer: 1, ..first };
        let pages = ActorArtworkPages {
            routes: Arc::new(BTreeMap::from([(rig, first)])),
            pack_source_locations: Arc::new(BTreeMap::from([(5, first), (6, second)])),
            pack_locations: Arc::new(BTreeSet::from([(1, 0), (1, 1)])),
            pages: Arc::from([ActorTexturePage {
                width: 1,
                height: 1,
                layers: 2,
                rgba8: [0; 8].into(),
                multitexture: false,
            }]),
            ..Default::default()
        };
        let route = pages.multitexture_location(rig, [5, 6, 6]).unwrap();
        assert_eq!(route.multitexture, Some([1, 1]));
        assert!(pages.valid(rig, route));
        assert!(pages.multitexture_location(rig, [5, 6, 5]).is_none());
        assert!(pages.multitexture_location(rig, [5, 7, 7]).is_none());
    }

    #[test]
    fn nearest_expansion_preserves_partial_alpha_and_normalized_texel_boundaries() {
        let texture = assets::ActorTexture {
            source: 0,
            width: 2,
            height: 1,
            pixel_sha256: [0; 32],
            rgba8: Arc::from([10, 20, 30, 2, 40, 50, 60, 255]),
        };
        let mut pixels = Vec::new();
        append_nearest(&mut pixels, &texture, 4, 2);
        assert_eq!(pixels.len(), 4 * 2 * 4);
        for (index, pixel) in pixels.as_chunks::<4>().0.iter().enumerate() {
            let source = (index % 4) / 2;
            assert_eq!(pixel, &texture.rgba8[source * 4..source * 4 + 4]);
        }
    }

    #[test]
    fn budgeted_replication_matches_expansion_then_box_filtering() {
        for (source_width, source_height, width, height) in [
            (1, 1, 16, 16),
            (8, 1, 8, 8),
            (1, 8, 8, 8),
            (2, 3, 7, 5),
            (5, 3, 5, 3),
            (3, 2, 9, 6),
        ] {
            let texture = assets::ActorTexture {
                source: 0,
                width: source_width,
                height: source_height,
                pixel_sha256: [0; 32],
                rgba8: (0..usize::from(source_width) * usize::from(source_height) * 4)
                    .map(|byte| (byte * 17) as u8)
                    .collect::<Vec<_>>()
                    .into(),
            };
            let mut expanded = Vec::new();
            append_nearest(&mut expanded, &texture, width, height);
            for limit in [1, 2, 3, 7, 16] {
                let expected = crate::fit_rgba_within(width, height, &expanded, limit)
                    .map_or_else(|| expanded.clone(), |(_, pixels)| pixels);
                assert_eq!(
                    replicated(&texture, width, height, limit).as_ref(),
                    expected,
                    "source {source_width}x{source_height}, page {width}x{height}, limit {limit}"
                );
            }
        }
    }

    #[test]
    fn grouped_texture_route_requires_native_material_and_same_page() {
        let location = |page, layer| ActorArtworkLocation {
            page,
            layer,
            pose_mode: assets::ActorPoseMode::CompiledLiteral,
            multitexture: None,
        };
        let first = location(1, 0);
        let pages = ActorArtworkPages {
            routes: Arc::new(BTreeMap::from([(EntityRigId(0), first)])),
            source_locations: Arc::new(BTreeMap::from([
                (5, first),
                (6, location(1, 1)),
                (7, location(1, 2)),
                (8, location(2, 0)),
            ])),
            entity_locations: Arc::new(BTreeSet::from([(1, 0), (1, 1), (1, 2), (2, 0)])),
            pages: Arc::from([ActorTexturePage {
                width: 1,
                height: 1,
                layers: 3,
                rgba8: [0; 12].into(),
                multitexture: true,
            }]),
            ..Default::default()
        };
        let route = pages
            .multitexture_location(EntityRigId(0), [5, 6, 7])
            .unwrap();
        assert_eq!(route.multitexture, Some([1, 2]));
        assert!(pages.valid(EntityRigId(0), route));
        assert!(
            pages
                .multitexture_location(EntityRigId(0), [5, 6, 8])
                .is_none()
        );
        assert!(!pages.valid(
            EntityRigId(0),
            ActorArtworkLocation {
                multitexture: Some([1, 3]),
                ..route
            }
        ));
        let mut ordinary = pages.clone();
        Arc::make_mut(&mut ordinary.pages)[0].multitexture = false;
        assert!(
            ordinary
                .multitexture_location(EntityRigId(0), [5, 6, 7])
                .is_none()
        );
    }

    #[test]
    fn pack_textures_a_multitexture_material_samples_share_one_page() {
        let texture = |source: u32, side: u16| assets::ActorTexture {
            source,
            width: side,
            height: side,
            pixel_sha256: [source as u8; 32],
            rgba8: vec![source as u8; usize::from(side) * usize::from(side) * 4].into(),
        };
        let binding = assets::ActorArtworkBinding {
            rig: 0,
            geometry_candidate: 0,
            entity_symbol: 0,
            geometry: 0,
            render_controller: 0,
            texture: 0,
            material: "llama".into(),
            pose_mode: assets::ActorPoseMode::CompiledLiteral,
        };
        let textures = [
            texture(10, 4),
            texture(11, 2),
            texture(12, 1),
            texture(13, 2),
        ];
        let pages = ActorArtworkPages::default().with_pack_artwork(
            &textures,
            &[binding],
            &assets::ActorMultitextureSources::from_draws([[10, 11, 12]]),
        );
        let rig = crate::pack_rig_id(0);
        let route = pages.multitexture_location(rig, [10, 11, 12]).unwrap();
        assert!(pages.valid(rig, route));
        let page = &pages.pages()[usize::from(route.page) - 1];
        assert!(page.multitexture);
        assert_eq!((page.width, page.height, page.layers), (4, 4, 3));
        let other = pages.variant_location(rig, 13).unwrap();
        assert_ne!(
            other.page, route.page,
            "unsampled textures keep their own pages"
        );
        assert!(!pages.pages()[usize::from(other.page) - 1].multitexture);
    }

    #[test]
    fn a_draws_textures_never_span_two_pages() {
        // Past one page of layers, the sampled textures of every draw stay together.
        const DRAWS: u32 = 100;
        let textures: Vec<_> = (0..DRAWS * 3)
            .map(|source| assets::ActorTexture {
                source,
                width: 1,
                height: 1,
                pixel_sha256: [0; 32],
                rgba8: Arc::from([source as u8, 0, 0, 255]),
            })
            .collect();
        let sources = assets::ActorMultitextureSources::from_draws(
            (0..DRAWS).map(|draw| [draw * 3, draw * 3 + 1, draw * 3 + 2]),
        );
        let pages = ActorArtworkPages::default().with_pack_artwork(&textures, &[], &sources);
        let page = |source: u32| pages.pack_source_locations[&source].page;
        for draw in sources.draws() {
            assert!(
                draw.iter().all(|source| page(*source) == page(draw[0])),
                "{draw:?}"
            );
        }
        assert!(pages.pages().len() > 1);
        assert!(pages.pages().iter().all(|page| page.multitexture
            && page.layers as usize <= super::super::page_budget::MAX_ACTOR_PAGE_LAYERS));
    }
}
