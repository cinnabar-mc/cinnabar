//! Resolution of characters a face lacks through its chain of attached fallbacks.

use super::*;

impl CompiledFontCatalog {
    /// Preserves the primary face's metrics and resolves missing characters through its
    /// fallback, then that face's fallback, and so on.
    pub fn glyph_source(&self, ch: char) -> Option<(&Self, GlyphMetrics)> {
        let mut face = self;
        loop {
            if let Some(glyph) = face.glyph(ch) {
                return Some((face, *glyph));
            }
            face = face.fallback.as_deref()?;
        }
    }

    /// A face with only this face's private-use glyphs, on the same pages and drawn the same
    /// way, without named faces, size alternatives, kerning or a fallback.
    pub fn private_use_subset(&self) -> Self {
        let glyphs: Arc<[GlyphMetrics]> = self
            .glyphs
            .iter()
            .filter(|glyph| crate::is_private_use_glyph(glyph.codepoint))
            .copied()
            .collect();
        let draw_sizes_64 = self
            .draw_sizes_64
            .iter()
            .filter(|(codepoint, _)| crate::is_private_use_glyph(**codepoint))
            .map(|(codepoint, size)| (*codepoint, *size))
            .collect();
        let mut hash = Sha256::new();
        hash.update(self.identity.carrier_sha256);
        hash.update(b"private-use subset");
        Self {
            identity: FontCatalogIdentity {
                carrier_sha256: hash.finalize().into(),
                ..self.identity
            },
            glyphs,
            pages: Arc::clone(&self.pages),
            draw_sizes_64: Arc::new(draw_sizes_64),
            named: Arc::default(),
            sizes: Arc::default(),
            linear_sampling: self.linear_sampling,
            rendering: self.rendering,
            line_metrics: self.line_metrics,
            kerning_64: Arc::default(),
            fallback: None,
        }
    }

    /// This face with `last(face)` appended to the end of its fallback chain and to each size
    /// alternative's; `last` receives the face it will serve and may decline with `None`.
    /// Appended glyphs must index pages every served face shares.
    pub fn with_last_fallback(&self, last: &impl Fn(&Self) -> Option<Arc<Self>>) -> Self {
        let mut result = self.clone();
        if let Some(face) = last(self) {
            result.append_fallback(face);
        }
        if !result.sizes.is_empty() {
            let sizes = Arc::make_mut(&mut result.sizes);
            for alternative in sizes.values_mut() {
                if let Some(face) = last(alternative) {
                    alternative.append_fallback(face);
                }
            }
        }
        result
    }

    /// Ends this face's fallback chain with `face` and changes its identity with it.
    fn append_fallback(&mut self, face: Arc<Self>) {
        fn chained(
            current: &CompiledFontCatalog,
            face: Arc<CompiledFontCatalog>,
        ) -> Arc<CompiledFontCatalog> {
            let mut current = current.clone();
            current.append_fallback(face);
            Arc::new(current)
        }
        let mut hash = Sha256::new();
        hash.update(self.identity.carrier_sha256);
        hash.update(b"appended fallback");
        hash.update(face.identity.carrier_sha256);
        self.identity.carrier_sha256 = hash.finalize().into();
        self.fallback = Some(match &self.fallback {
            Some(next) => chained(next, face),
            None => face,
        });
    }
}
