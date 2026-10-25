//! Page sizing: same-size rasters share capped array pages within the actor pixel budget.

use super::{
    ActorArtworkPageId, ActorTexturePage, MAX_ACTOR_GPU_PIXEL_BYTES, MAX_ACTOR_TEXTURE_PAGES,
};
use std::{collections::BTreeMap, sync::Arc};

/// Layers per generic entity page, within every backend's array-layer limit.
pub(super) const MAX_ACTOR_PAGE_LAYERS: usize = 256;
/// One page is one GPU allocation; large rasters split so a failed or replaced page stays small.
pub(super) const MAX_ACTOR_PAGE_BYTES: usize = 32 * 1024 * 1024;

/// Layers per page for rasters of one size; a raster larger than the cap gets its own page.
pub(super) fn page_layers(width: u16, height: u16) -> usize {
    let layer = usize::from(width) * usize::from(height) * 4;
    (MAX_ACTOR_PAGE_BYTES / layer.max(1)).clamp(1, MAX_ACTOR_PAGE_LAYERS)
}

pub(super) fn within_page_budget(generic_pages: usize, declared_pixel_bytes: usize) -> bool {
    generic_pages < MAX_ACTOR_TEXTURE_PAGES && declared_pixel_bytes <= MAX_ACTOR_GPU_PIXEL_BYTES
}

/// Appends `page`, box-filtered down until it fits the byte budget, and returns its page id;
/// `None` only once every page id is taken.
pub(super) fn push_page(
    pages: &mut Vec<ActorTexturePage>,
    gpu_bytes: &mut usize,
    page: ActorTexturePage,
) -> Option<ActorArtworkPageId> {
    let mut page = page;
    while !within_page_budget(pages.len() + 1, gpu_bytes.saturating_add(page.rgba8.len())) {
        let longest = u32::from(page.width.max(page.height));
        if pages.len() + 1 >= MAX_ACTOR_TEXTURE_PAGES || longest <= 1 {
            return None;
        }
        page = page.fit_within(longest / 2).into_owned();
    }
    *gpu_bytes += page.rgba8.len();
    pages.push(page);
    ActorArtworkPageId::try_from(pages.len()).ok()
}

/// Reorders `members` so every member with a `companion` key follows the first member with the
/// same key; members without one keep their order.
pub(super) fn gather_companions(members: &mut [usize], companion: impl Fn(usize) -> Option<u32>) {
    let mut first = BTreeMap::new();
    let mut ordered: Vec<(usize, usize)> = members
        .iter()
        .enumerate()
        .map(|(position, &member)| {
            let lead =
                companion(member).map_or(position, |key| *first.entry(key).or_insert(position));
            (lead, member)
        })
        .collect();
    ordered.sort_by_key(|(lead, _)| *lead);
    for (slot, (_, member)) in members.iter_mut().zip(ordered) {
        *slot = member;
    }
}

/// Splits `members` into page chunks of at most `layers`. A run of adjacent members `together`
/// joins is never split across pages: it moves whole to the next page, and a run larger than
/// `layers` takes pages of its own at the backend layer limit, past the byte cap if it must.
pub(super) fn page_chunks(
    members: &[usize],
    layers: usize,
    together: impl Fn(usize, usize) -> bool,
) -> Vec<&[usize]> {
    let mut chunks = Vec::new();
    let (mut open, mut run) = (0, 0);
    for end in 1..=members.len() {
        if end < members.len() && together(members[end - 1], members[end]) {
            continue;
        }
        if end - open > layers && open < run {
            chunks.push(&members[open..run]);
            open = run;
        }
        if end - open > layers {
            chunks.extend(members[open..end].chunks(MAX_ACTOR_PAGE_LAYERS));
            open = end;
        }
        run = end;
    }
    if open < members.len() {
        chunks.push(&members[open..]);
    }
    chunks
}

/// Pushes a same-size group split into capped pages, all at the one resolution the remaining
/// budget allows for the whole group. Adjacent members `together` joins stay on one page (see
/// [`page_chunks`]). Each chunk is built, fitted and placed before the next is built, so at most
/// one full-resolution chunk is alive. `placed` receives each admitted chunk, its page id,
/// `build`'s value, and whether the page kept the built pixels unchanged.
#[allow(clippy::too_many_arguments)]
pub(super) fn push_group<'a, T>(
    pages: &mut Vec<ActorTexturePage>,
    gpu_bytes: &mut usize,
    members: &'a [usize],
    [width, height]: [u16; 2],
    multitexture: bool,
    together: impl Fn(usize, usize) -> bool,
    mut build: impl FnMut(&'a [usize]) -> (ActorTexturePage, T),
    mut placed: impl FnMut(&'a [usize], ActorArtworkPageId, T, bool),
) {
    let mut limit = u32::from(width.max(height));
    let [fitted_width, fitted_height] = loop {
        let fitted = crate::actor::texel_fit::fitted_dimensions(width, height, limit);
        let bytes = usize::from(fitted[0]) * usize::from(fitted[1]) * 4 * members.len();
        if gpu_bytes.saturating_add(bytes) <= MAX_ACTOR_GPU_PIXEL_BYTES || limit <= 1 {
            break fitted;
        }
        limit /= 2;
    };
    // Three-sampler layers must share a page, so those keep the layer limit alone.
    let layers = if multitexture {
        MAX_ACTOR_PAGE_LAYERS
    } else {
        page_layers(fitted_width, fitted_height)
    };
    for chunk in page_chunks(members, layers, together) {
        let (built, value) = build(chunk);
        let page = built.fit_within(limit).into_owned();
        // Only unchanged pixels stay referenced; a downscaled chunk's original frees here.
        let unchanged = Arc::ptr_eq(&built.rgba8, &page.rgba8).then(|| Arc::clone(&page.rgba8));
        drop(built);
        if let Some(id) = push_page(pages, gpu_bytes, page) {
            let kept = unchanged
                .is_some_and(|pixels| Arc::ptr_eq(&pixels, &pages[usize::from(id) - 1].rgba8));
            placed(chunk, id, value, kept);
        }
    }
}
