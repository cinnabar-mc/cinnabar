//! The two authored Enhanced pages, full and low resolution, as colour, normal and material
//! arrays built layer by layer.

use assets::texture::{PBR_TILE_SIZE, TILE_SIZE, TextureArray, TextureMip};
use image::{ImageBuffer, Rgba};

use super::{PbrMipLayer, PbrSurface, build_pbr_mips};

/// Tile side of the low-resolution page; authored frames this size or smaller use it.
pub const LOW_PBR_TILE_SIZE: u32 = TILE_SIZE * 2;
/// Authored layers sit below the `PBR_REF_*` flag bits of their packed reference.
pub const MAX_AUTHORED_LAYERS: u32 = assets::texture::PBR_REF_COLOR;

/// One page's layers in mip order.
struct Page {
    size: u32,
    layers: u32,
    data: Vec<Vec<u8>>,
}

impl Page {
    fn new(size: u32) -> Self {
        Self {
            size,
            layers: 0,
            data: (0..=size.ilog2()).map(|_| Vec::new()).collect(),
        }
    }

    fn append(&mut self, mips: &[TextureMip]) {
        for (data, mip) in self.data.iter_mut().zip(mips) {
            data.extend_from_slice(&mip.rgba8);
        }
        self.layers += 1;
    }

    fn finish(self) -> TextureArray {
        TextureArray {
            layers: self.layers,
            mips: self
                .data
                .into_iter()
                .enumerate()
                .map(|(level, data)| TextureMip {
                    size: self.size >> level,
                    rgba8: data.into_boxed_slice(),
                })
                .collect(),
        }
    }
}

/// Colour, normal and material arrays of both authored pages.
pub struct PbrPages {
    color: [Page; 2],
    normal: [Page; 2],
    material: [Page; 2],
}

impl Default for PbrPages {
    fn default() -> Self {
        let pages = || [Page::new(PBR_TILE_SIZE), Page::new(LOW_PBR_TILE_SIZE)];
        Self {
            color: pages(),
            normal: pages(),
            material: pages(),
        }
    }
}

/// The three arrays of both pages, in colour, normal, material order.
pub type PbrPageArrays = [[TextureArray; 2]; 3];

impl PbrPages {
    /// The page an authored frame of side `native` uploads to.
    #[must_use]
    pub fn page_for(native: u32) -> usize {
        usize::from(native <= LOW_PBR_TILE_SIZE)
    }

    /// The tile side of `page`.
    #[must_use]
    pub fn tile_size(&self, page: usize) -> u32 {
        self.color[page].size
    }

    /// Layers `page` holds.
    #[must_use]
    pub fn layers(&self, page: usize) -> u32 {
        self.color[page].layers
    }

    /// Whether `page` can take another layer.
    #[must_use]
    pub fn has_room(&self, page: usize) -> bool {
        self.layers(page) < MAX_AUTHORED_LAYERS
    }

    /// Appends `layer` to `page` and returns its packed authored reference: the page in the top
    /// bit, the layer, and the layer's PBR flags.
    pub fn push(&mut self, page: usize, layer: &PbrMipLayer) -> u32 {
        let index = self.color[page].layers;
        self.color[page].append(&layer.color);
        self.normal[page].append(&layer.normal);
        self.material[page].append(&layer.material);
        ((page as u32) << 31) | index | layer.flags
    }

    /// The upload bytes of one layer of `page` across its three arrays and mip chains.
    #[must_use]
    pub fn layer_bytes(&self, page: usize) -> usize {
        let mut bytes = 0;
        let mut size = self.tile_size(page) as usize;
        while size > 0 {
            bytes += size * size * 4;
            size /= 2;
        }
        bytes * 3
    }

    /// The finished arrays, or `None` without any layer. An empty page receives one neutral
    /// layer of side `fallback(page)`, as every page must hold one.
    #[must_use]
    pub fn finish(mut self, fallback: impl Fn(usize) -> u32) -> Option<PbrPageArrays> {
        if self.color.iter().all(|page| page.layers == 0) {
            return None;
        }
        for page in 0..2 {
            if self.color[page].layers == 0 {
                let size = fallback(page);
                let neutral = neutral_layer(size);
                for (arrays, mips) in [
                    (&mut self.color, &neutral.color),
                    (&mut self.normal, &neutral.normal),
                    (&mut self.material, &neutral.material),
                ] {
                    arrays[page] = Page::new(size);
                    arrays[page].append(mips);
                }
            }
        }
        Some([
            self.color.map(Page::finish),
            self.normal.map(Page::finish),
            self.material.map(Page::finish),
        ])
    }
}

/// A neutral authored layer of side `size`: mid grey, flat normal, default material.
#[must_use]
pub fn neutral_layer(size: u32) -> PbrMipLayer {
    let image = |pixel| ImageBuffer::from_pixel(1, 1, Rgba(pixel));
    build_pbr_mips(
        &PbrSurface {
            color: image([128, 128, 128, 255]),
            normal: image([128, 128, 255, 128]),
            material: image([0, 0, 255, 0]),
            flags: 0,
        },
        size,
        false,
        false,
    )
    .expect("constant authored fallback is valid")
}
