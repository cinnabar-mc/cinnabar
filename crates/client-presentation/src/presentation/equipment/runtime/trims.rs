//! Visible armor compositions share source pixels and retire with the frame's equipment.
use super::*;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    trim: protocol::ItemTrim,
    item: Arc<str>,
    source: usize,
    layer: u32,
    tint: u32,
    icon: Option<[u16; 4]>,
}

struct Image {
    _source: render_model::PagePixels,
    raster: EquipmentRaster,
    used: bool,
}

#[derive(Default)]
pub(super) struct Trims {
    images: BTreeMap<Key, Image>,
}

impl Trims {
    pub(super) fn begin_frame(&mut self) {
        for image in self.images.values_mut() {
            image.used = false;
        }
    }
    pub(super) fn finish_frame(&mut self) {
        self.images.retain(|_, image| image.used);
    }
    pub(super) fn clear(&mut self) {
        self.images.clear();
    }
}

impl EquipmentRuntime {
    /// Decorates the selected armor sheet or held armor sprite, preserving the mesh's UV frame.
    pub(crate) fn trim_raster(
        &mut self,
        item: &WornItem,
        artwork: &ActorArtworkPages,
        location: ActorArtworkLocation,
        rig: EntityRigId,
        tint: u32,
        worn: bool,
    ) -> Option<(EquipmentRaster, EntityRigId)> {
        let trim = item.trim.as_ref()?;
        let slot = assets::item::trim::armor_slot(&item.identifier)?;
        let icon = if worn {
            None
        } else {
            match self.meshes.iter().find(|(_, cached)| **cached == Some(rig)) {
                Some((MeshKey::Sprite(index), _)) => Some(*self.placements.get(*index)?.as_ref()?),
                _ => return None,
            }
        };
        let page = artwork
            .pages()
            .get(usize::from(location.page()).checked_sub(1)?)?;
        let pixels = page.shared_pixels();
        let (width, height) = page.dimensions();
        let key = Key {
            trim: trim.clone(),
            item: Arc::clone(&item.identifier),
            source: pixels.storage_id(),
            layer: location.layer(),
            tint,
            icon: icon.map(|p| [p.x, p.y, p.width, p.height]),
        };
        if !self.trims.images.contains_key(&key) {
            let bytes = usize::from(width) * usize::from(height) * 4;
            let start = usize::try_from(location.layer()).ok()?.checked_mul(bytes)?;
            let selected = pixels.get(start..start.checked_add(bytes)?)?;
            let [x, y, w, h] = key.icon.unwrap_or([0, 0, width, height]);
            if u32::from(x) + u32::from(w) > u32::from(width)
                || u32::from(y) + u32::from(h) > u32::from(height)
            {
                return None;
            }
            let mut base_pixels = Vec::with_capacity(usize::from(w) * usize::from(h) * 4);
            for row in y..y + h {
                let start = (usize::from(row) * usize::from(width) + usize::from(x)) * 4;
                base_pixels.extend_from_slice(&selected[start..start + usize::from(w) * 4]);
            }
            let startup = self.catalog.as_deref()?;
            let pack = self.pack.as_ref().and_then(|pack| pack.catalog.as_deref());
            let binding = pack
                .and_then(|c| c.binding(&item.identifier))
                .or_else(|| startup.binding(&item.identifier))?;
            let base = assets::EquipmentTexture {
                identifier: binding.texture.identifier.clone(),
                width: w,
                height: h,
                rgba8: base_pixels.into(),
            };
            let dye = (tint != 0)
                .then_some(((tint & 255) << 16) | (tint & 0xff00) | ((tint >> 16) & 255));
            let composed = assets::item::trim::compose(
                &base,
                &trim.material,
                &trim.pattern,
                slot,
                icon.is_some(),
                dye,
                Some(&binding.resources),
                |path| {
                    pack.and_then(|c| c.texture(path))
                        .or_else(|| startup.texture(path))
                },
            )?;
            let raster = if key.icon.is_none() {
                EquipmentRaster {
                    width: composed.width,
                    height: composed.height,
                    rgba8: composed.rgba8,
                }
            } else {
                let mut output = selected.to_vec();
                for row in 0..h {
                    for column in 0..w {
                        let sx = usize::from(column) * usize::from(composed.width) / usize::from(w);
                        let sy = usize::from(row) * usize::from(composed.height) / usize::from(h);
                        let at = (usize::from(row + y) * usize::from(width)
                            + usize::from(column + x))
                            * 4;
                        let from = (sy * usize::from(composed.width) + sx) * 4;
                        output[at..at + 4].copy_from_slice(&composed.rgba8[from..from + 4]);
                    }
                }
                EquipmentRaster {
                    width,
                    height,
                    rgba8: output.into(),
                }
            };
            self.trims.images.insert(
                key.clone(),
                Image {
                    _source: pixels,
                    raster,
                    used: true,
                },
            );
        }
        let image = self.trims.images.get_mut(&key)?;
        image.used = true;
        Some((image.raster.clone(), rig))
    }
}
