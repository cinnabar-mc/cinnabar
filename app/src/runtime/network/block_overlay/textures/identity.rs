//! Immutable pixel and destination identities used for texture reuse.

use super::FlipbookSource;

/// Pixel and animation policy shared by equivalent catalog and recorded-path routes.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(in super::super) struct TextureIdentity {
    pub(in super::super) path: TexturePath,
    pub(in super::super) tint: Option<[u8; 3]>,
    pub(in super::super) grid: u8,
    pub(in super::super) flipbook: Option<FlipbookSource>,
    pub(in super::super) selector: (u32, u32),
}

/// Keeps logical aliases separate from resolved filenames that happen to share their spelling.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(in super::super) enum TexturePath {
    Catalog(String),
    Image(String),
}

/// Pixel-storage identity, independent of animation timing and atlas destinations.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(in super::super) struct TextureStorageKey {
    pub(in super::super) path: TexturePath,
    /// Keeps authored surfaces separate even when their colour images are shared.
    pub(in super::super) texture_set: Option<String>,
    pub(in super::super) grid: u8,
    pub(in super::super) tint: Option<[u8; 3]>,
    pub(in super::super) frames: Option<(Box<[u32]>, u32)>,
}

impl TextureStorageKey {
    /// Uses the actual image selected by decoding while retaining its sampling policy.
    pub(in super::super) fn with_image(mut self, path: String) -> Self {
        self.path = TexturePath::Image(path);
        self
    }
}
