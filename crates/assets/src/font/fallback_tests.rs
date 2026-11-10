//! Fallback chains resolve each face in order, and extending a chain shares glyph tables.

use super::*;

/// A one-page catalog drawing `codepoints`, each as a one-texel glyph.
fn catalog(codepoints: &[char], seed: u8) -> CompiledFontCatalog {
    let pixels = vec![seed; 16 * 16].into_boxed_slice();
    let page = FontTexturePage {
        source_path: format!("font/fixture-{seed}.png").into(),
        source_bytes: 1,
        source_sha256: [seed; 32],
        pixels_sha256: Sha256::digest(&pixels).into(),
        width: 16,
        height: 16,
        pixels: FontPixels::Coverage(pixels.into()),
    };
    let glyphs: Vec<_> = codepoints
        .iter()
        .map(|&codepoint| GlyphMetrics {
            codepoint,
            page: 0,
            uv: [0, 0, 1, 1],
            bearing: [0, -1],
            advance_64: 64,
        })
        .collect();
    let bytes = encode_font_catalog([seed; 32], &glyphs, &[page]).unwrap();
    CompiledFontCatalog::decode(&bytes, [seed; 32]).unwrap()
}

#[test]
fn appended_fallbacks_resolve_last_and_share_the_chain_glyph_tables() {
    let sans = Arc::new(catalog(&['B'], 2));
    let mut primary = catalog(&['A'], 1);
    primary.fallback = Some(Arc::clone(&sans));
    let mut alternative = primary.clone();
    alternative.line_metrics = Some(FontLineMetrics {
        em_64: 64,
        ascent_64: 48,
        descent_64: 16,
    });
    primary.sizes = Arc::new(BTreeMap::from([(1, alternative)]));
    let icons = Arc::new(catalog(&['A', '\u{e000}'], 3).private_use_subset());
    assert!(icons.glyph('A').is_none());
    assert!(primary.glyph_source('\u{e000}').is_none());

    let served = std::cell::Cell::new(0);
    let extended = primary.with_last_fallback(&|_| {
        served.set(served.get() + 1);
        Some(Arc::clone(&icons))
    });
    assert_eq!(served.get(), 2, "the face and its size alternative");
    for face in [&extended, &extended.sizes[&1]] {
        assert!(std::ptr::eq(face.glyph_source('A').unwrap().0, face));
        let (source, _) = face.glyph_source('B').unwrap();
        // The chained copy of the shared fallback keeps the same glyph table.
        assert!(Arc::ptr_eq(&source.glyphs, &sans.glyphs));
        let (source, _) = face.glyph_source('\u{e000}').unwrap();
        assert!(std::ptr::eq(source, &*icons));
        assert_ne!(face.identity(), primary.identity());
        assert!(Arc::ptr_eq(&face.glyphs, &primary.glyphs));
    }
}
