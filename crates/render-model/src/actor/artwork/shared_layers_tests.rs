//! Packed session artwork references its compiled textures instead of keeping a second copy.
use super::*;

/// Distinct `side`×`side` textures for `sources`, so no two share pixels or a page-cache entry.
fn distinct_textures(sources: std::ops::Range<u32>, side: u16) -> Vec<assets::ActorTexture> {
    sources
        .map(|source| {
            let mut pixels: Vec<u8> = (0..usize::from(side) * usize::from(side) * 4)
                .map(|byte| byte as u8)
                .collect();
            pixels[..4].copy_from_slice(&source.to_le_bytes());
            let rgba8: Arc<[u8]> = pixels.into();
            assets::ActorTexture {
                source,
                width: side,
                height: side,
                pixel_sha256: Sha256::digest(&rgba8).into(),
                rgba8,
            }
        })
        .collect()
}

/// Bytes of every distinct allocation the textures and pages hold, counted once each.
fn retained_bytes(textures: &[assets::ActorTexture], pages: &ActorArtworkPages) -> usize {
    let mut seen = BTreeSet::new();
    textures
        .iter()
        .map(|texture| &texture.rgba8)
        .chain(pages.pages().iter().flat_map(|page| page.rgba8.parts()))
        .filter(|part| seen.insert(part.as_ptr()))
        .map(|part| part.len())
        .sum()
}

#[test]
fn reused_pixels_keep_their_layer_boundaries() {
    let layers = distinct_textures(0..2, 32);
    let live = ActorArtworkPages::default().with_pack_artwork(&layers, &[]);
    assert_eq!(live.pages()[0].layers(), 2);

    // The same bytes as one taller layer must not borrow the live page's two allocations.
    let rgba8: Arc<[u8]> = [&*layers[0].rgba8, &*layers[1].rgba8].concat().into();
    let tall = assets::ActorTexture {
        source: 2,
        width: 32,
        height: 64,
        pixel_sha256: Sha256::digest(&rgba8).into(),
        rgba8,
    };
    let tall = ActorArtworkPages::default().with_pack_artwork(std::slice::from_ref(&tall), &[]);
    let page = &tall.pages()[0];
    assert_eq!((page.layers(), page.rgba8.parts().len()), (1, 1));
    let fitted = page.fit_within(32);
    assert_eq!(fitted.dimensions(), (16, 32));
    assert_eq!(fitted.rgba8.len(), 16 * 32 * 4);

    // A page with the same layers still reuses the live allocations.
    let again = distinct_textures(0..2, 32);
    let again = ActorArtworkPages::default().with_pack_artwork(&again, &[]);
    assert!(again.pages()[0].rgba8.ptr_eq(&live.pages()[0].rgba8));
}

#[test]
fn packed_pack_artwork_retains_one_copy_of_each_texel() {
    // More same-size layers than one page holds, plus a second size, so grouping and splitting
    // both run.
    let mut textures = distinct_textures(0..300, 32);
    textures.extend(distinct_textures(300..305, 64));
    let texels: usize = textures
        .iter()
        .map(|texture| usize::from(texture.width) * usize::from(texture.height))
        .sum();
    let (pages, allocated) =
        test_alloc::measure(|| ActorArtworkPages::default().with_pack_artwork(&textures, &[]));
    assert!(pages.pages().len() >= 3);
    assert_eq!(
        pages.pages().iter().map(|page| page.layers()).sum::<u32>(),
        textures.len() as u32
    );
    assert_eq!(retained_bytes(&textures, &pages), texels * 4);
    // Page metadata stays well under one byte per texel; a pixel copy costs four.
    assert!(
        allocated.bytes < texels,
        "packing allocated {} bytes for {texels} texels",
        allocated.bytes
    );
    let rasters: Vec<_> = textures
        .iter()
        .map(|texture| EquipmentRaster {
            width: texture.width,
            height: texture.height,
            rgba8: Arc::clone(&texture.rgba8),
        })
        .collect();
    let (equipment, _) = ActorArtworkPages::default().with_equipment_rasters(&rasters);
    assert_eq!(retained_bytes(&textures, &equipment), texels * 4);
}
