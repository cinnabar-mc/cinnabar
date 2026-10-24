//! Turns the client parts' retained world material declarations into the set the renderer
//! draws: each shader validated again, each declared texture decoded, every material charged
//! to its bundle's GPU share, and every binding resolved to a slot.

use mod_render::world_material::{
    self as abi,
    publication::{Blend, Depth, Published, Texture, WorldMaterialSet},
};
use server_experience::world_material::{
    WorldBlend, WorldDepth, WorldMaterialSpec, WorldMaterials, WorldTarget,
};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    io::Cursor,
    sync::Arc,
};

/// Provisional: the widest or tallest declared texture, in texels.
pub(super) const MAX_TEXTURE_EXTENT: u32 = 1024;
/// Provisional: GPU bytes charged per material for its pipelines and bind group, on top of its
/// textures.
pub(super) const PIPELINE_CHARGE_BYTES: u64 = 256 * 1024;
/// Failure descriptions kept for the session's diagnostics, newest last.
pub(super) const MAX_DIAGNOSTICS: usize = 16;

/// One bundle's declarations as the presenter reads them.
pub(super) struct BundleWorld<'a> {
    pub bundle: &'a str,
    pub world: &'a WorldMaterials,
    /// The bundle's share of the signed GPU limit.
    pub gpu_bytes: u64,
    /// The bundle's signed PNGs by path.
    pub textures: &'a [(String, Vec<u8>)],
}

/// A declaration's last outcome, kept while the declaration is unchanged.
struct Cached {
    spec: WorldMaterialSpec,
    /// Numbers this admission attempt, so a failure is reported once per attempt.
    attempt: u64,
    outcome: Result<Published, String>,
}

#[derive(Default)]
pub(super) struct WorldMaterialPresenter {
    cache: BTreeMap<(String, u32), Cached>,
    next_key: u64,
    revision: u64,
    set: Arc<WorldMaterialSet>,
    /// Failures already reported, so each is logged once.
    reported: BTreeSet<(String, u32, u64, String)>,
    diagnostics: VecDeque<String>,
}

impl WorldMaterialPresenter {
    /// The set to draw.
    pub(super) fn set(&self) -> &Arc<WorldMaterialSet> {
        &self.set
    }

    /// The most recent failures, oldest first, at most [`MAX_DIAGNOSTICS`].
    pub(super) fn diagnostics(&self) -> impl Iterator<Item = &str> {
        self.diagnostics.iter().map(String::as_str)
    }

    /// Rebuilds the set from every bundle's declarations; returns whether it changed. An
    /// unchanged declaration keeps its compiled material and key.
    pub(super) fn rebuild<'a>(
        &mut self,
        bundles: impl IntoIterator<Item = BundleWorld<'a>>,
    ) -> bool {
        let mut materials = Vec::new();
        let mut entities = BTreeMap::new();
        let mut blocks = BTreeMap::new();
        let mut live = BTreeSet::new();
        for bundle in bundles {
            let mut remaining = bundle.gpu_bytes;
            let mut slots = BTreeMap::new();
            for (&id, spec) in &bundle.world.materials {
                let key = (bundle.bundle.to_owned(), id);
                live.insert(key.clone());
                let (attempt, outcome) = match self.cache.get(&key) {
                    Some(cached) if cached.spec == *spec => {
                        (cached.attempt, cached.outcome.clone())
                    }
                    _ => {
                        self.next_key += 1;
                        let outcome = admit(self.next_key, spec, bundle.textures);
                        self.cache.insert(
                            key.clone(),
                            Cached {
                                spec: spec.clone(),
                                attempt: self.next_key,
                                outcome: outcome.clone(),
                            },
                        );
                        (self.next_key, outcome)
                    }
                };
                let published = match outcome {
                    Ok(published) => published,
                    Err(reason) => {
                        self.report(bundle.bundle, id, attempt, &reason);
                        continue;
                    }
                };
                let charge = published
                    .textures
                    .iter()
                    .map(Texture::bytes)
                    .sum::<u64>()
                    .saturating_add(PIPELINE_CHARGE_BYTES);
                if charge > remaining {
                    self.report(bundle.bundle, id, attempt, "exceeds the bundle's GPU limit");
                    continue;
                }
                remaining -= charge;
                let Ok(slot) = u16::try_from(materials.len()) else {
                    continue;
                };
                materials.push(published);
                slots.insert(id, slot);
            }
            for binding in bundle.world.bindings.values() {
                let Some(&slot) = slots.get(&binding.material) else {
                    continue;
                };
                let (map, identifier) = match &binding.target {
                    WorldTarget::Entity(identifier) => (&mut entities, identifier),
                    WorldTarget::Block(identifier) => (&mut blocks, identifier),
                };
                // Two bundles may sign one namespace; the first in bundle order keeps it.
                map.entry(identifier.as_str().into()).or_insert(slot);
            }
        }
        self.cache.retain(|key, _| live.contains(key));
        let set = WorldMaterialSet {
            revision: self.revision,
            materials,
            entities,
            blocks,
        };
        if set.materials == self.set.materials
            && set.entities == self.set.entities
            && set.blocks == self.set.blocks
        {
            return false;
        }
        self.revision += 1;
        self.set = Arc::new(WorldMaterialSet {
            revision: self.revision,
            ..set
        });
        true
    }

    /// Logs a failure once per declaration attempt and keeps it for the diagnostics.
    fn report(&mut self, bundle: &str, id: u32, attempt: u64, reason: &str) {
        if !self
            .reported
            .insert((bundle.to_owned(), id, attempt, reason.to_owned()))
        {
            return;
        }
        bevy::log::warn!(bundle, id, reason, "world material not drawn");
        if self.diagnostics.len() == MAX_DIAGNOSTICS {
            self.diagnostics.pop_front();
        }
        self.diagnostics
            .push_back(format!("{bundle} world material {id}: {reason}"));
        // Attempts no longer cached cannot recur, so their entries go with them.
        let cache = &self.cache;
        self.reported.retain(|(bundle, id, attempt, _)| {
            cache
                .get(&(bundle.clone(), *id))
                .is_some_and(|cached| cached.attempt == *attempt)
        });
    }
}

/// Validates the shader again and decodes the declared textures; the helper's validation is
/// not trusted.
fn admit(
    key: u64,
    spec: &WorldMaterialSpec,
    textures: &[(String, Vec<u8>)],
) -> Result<Published, String> {
    let material = abi::compose(&spec.source, spec.textures.len())?;
    let decoded = spec
        .textures
        .iter()
        .map(|path| {
            let bytes = textures
                .iter()
                .find(|(listed, _)| listed == path)
                .map(|(_, bytes)| bytes.as_slice())
                .ok_or_else(|| format!("{path} is not a signed texture"))?;
            decode(bytes).ok_or_else(|| format!("{path} is not a bounded PNG"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut params = [0.0; 16];
    for (slot, value) in params.iter_mut().zip(&spec.params) {
        *slot = *value;
    }
    Ok(Published {
        key,
        material: Arc::new(material),
        textures: decoded.into(),
        params,
        depth: match spec.depth {
            WorldDepth::Test => Depth::Test,
            WorldDepth::Always => Depth::Always,
        },
        blend: match spec.blend {
            WorldBlend::Opaque => Blend::Opaque,
            WorldBlend::Cutout => Blend::Cutout,
            WorldBlend::Premultiplied => Blend::Premultiplied,
            WorldBlend::Additive => Blend::Additive,
        },
    })
}

/// Decodes a PNG of at most [`MAX_TEXTURE_EXTENT`] texels a side to RGBA8.
fn decode(bytes: &[u8]) -> Option<Texture> {
    let mut reader = image::ImageReader::with_format(Cursor::new(bytes), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_TEXTURE_EXTENT);
    limits.max_image_height = Some(MAX_TEXTURE_EXTENT);
    limits.max_alloc = Some(u64::from(MAX_TEXTURE_EXTENT).pow(2) * 8);
    reader.limits(limits);
    let image = reader.decode().ok()?.to_rgba8();
    Some(Texture {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw().into(),
    })
}

#[cfg(test)]
mod tests;
