use super::{UiRenderRejectReason, UiTextureBucket, UiTextureFormat, page_bytes_in};
use std::sync::Arc;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct UiMip {
    pub dimensions: [u32; 2],
    pub pixels: Box<[u8]>,
}

/// Averages each smaller terrain level directly from its source texels.
pub(super) fn terrain_levels(
    pixels: &[u8],
    dimensions: [u32; 2],
) -> Result<Arc<[UiMip]>, UiRenderRejectReason> {
    let count = assets::texture::VANILLA_TERRAIN_MIP_COUNT.min(
        dimensions
            .into_iter()
            .max()
            .unwrap_or(0)
            .checked_ilog2()
            .unwrap_or(0)
            + 1,
    );
    (1..count)
        .map(|level| {
            let (pixels, dimensions) =
                assets::texture::legacy_terrain_mip(pixels, dimensions, level)
                    .map_err(|_| UiRenderRejectReason::InvalidTextureExtent)?;
            Ok(UiMip { dimensions, pixels })
        })
        .collect::<Result<Arc<[_]>, _>>()
}

/// Counts validated texture storage, including every smaller level.
pub(super) fn storage_bytes(
    dimensions: [u32; 2],
    format: UiTextureFormat,
    levels: u32,
) -> Result<usize, UiRenderRejectReason> {
    let source_bytes = page_bytes_in(dimensions, format)?;
    let maximum = dimensions
        .into_iter()
        .max()
        .unwrap_or(0)
        .checked_ilog2()
        .unwrap_or(0)
        + 1;
    if levels == 0 || levels > maximum {
        return Err(UiRenderRejectReason::InvalidTextureExtent);
    }
    (1..levels).try_fold(source_bytes, |total, level| {
        total
            .checked_add(page_bytes_in(
                dimensions.map(|side| (side >> level).max(1)),
                format,
            )?)
            .ok_or(UiRenderRejectReason::InvalidTextureExtent)
    })
}

impl UiTextureBucket {
    /// Allocation storage for every layer and mip of this validated bucket.
    pub fn byte_len(&self) -> usize {
        storage_bytes(self.dimensions, self.format, self.mip_level_count)
            .expect("UI texture buckets retain validated dimensions")
            * self.layers as usize
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        UiRenderRejectReason, UiTextureCatalog, UiTextureFormat, UiTexturePage, UiTexturePlan,
    };

    #[test]
    fn planning_rejects_zero_source_extents_in_every_storage_format() {
        for dimensions in [[0, 0], [0, 256], [256, 0]] {
            for format in [UiTextureFormat::Rgba8, UiTextureFormat::Coverage] {
                assert_eq!(
                    UiTexturePlan::with_formats(&[(dimensions, format)]),
                    Err(UiRenderRejectReason::InvalidTextureExtent),
                );
            }
        }
    }

    #[test]
    fn filtered_pages_keep_their_source_and_account_for_every_resident_level() {
        let plain = UiTexturePage::owned([16; 2], vec![255; 16 * 16 * 4].into()).unwrap();
        let filtered = plain.clone().with_terrain_mips().unwrap();
        assert_eq!(filtered.pixels(), plain.pixels());
        assert_ne!(filtered.identity(), plain.identity());
        assert_eq!(
            filtered.clone().with_terrain_mips().unwrap().identity(),
            filtered.identity(),
        );
        let bytes = plain.byte_len() + filtered.byte_len();
        assert_eq!(
            filtered.byte_len(),
            filtered
                .mip_levels()
                .map(|(_, pixels)| pixels.len())
                .sum::<usize>(),
        );
        let catalog = UiTextureCatalog::new(vec![plain, filtered], 2).unwrap();
        assert_eq!(catalog.plan().bytes(), bytes);
        assert_ne!(
            catalog.plan().locations()[0].bucket,
            catalog.plan().locations()[1].bucket,
            "point-only pages cannot alias mipmapped GPU storage",
        );
    }
}
