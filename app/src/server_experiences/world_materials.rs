//! Turns the client parts' retained world material declarations into the set the renderer
//! draws: each shader validated again, each declared texture decoded, every material charged
//! to its bundle's GPU share, and every binding resolved to a slot.

use mod_render::world_material::{
    self as abi,
    publication::{self, Blend, Depth, Published, Texture, WorldMaterialSet},
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
    /// The bundle's share of the signed memory limit, which the host's copies of its shader
    /// sources, parameters and decoded textures draw from.
    pub memory_bytes: u64,
    /// The bundle's signed PNGs by path.
    pub textures: &'a [(String, Vec<u8>)],
}

/// A declaration's last outcome, kept while its program is unchanged.
struct Cached {
    spec: WorldMaterialSpec,
    /// Numbers this admission attempt, so a failure is reported once per attempt; it is also
    /// the material's key.
    attempt: u64,
    outcome: Outcome,
    /// Whether this attempt's material ever reached the drawn set, and so may hold compiled
    /// pipelines that replacing or removing it leaves charged.
    published: bool,
}

/// What admitting a declaration last produced.
enum Outcome {
    /// Drawable, with its decoded textures.
    Ready(Published),
    /// A valid shader whose charge did not fit its bundle's remaining shares when last tried.
    /// It holds no decoded texels; its textures decode once the charge fits.
    Waiting {
        material: Arc<abi::WorldMaterial>,
        /// Bytes its declared textures decode to, read from their headers.
        texels: u64,
    },
    /// Refused for this reason until its program changes.
    Refused(String),
}

/// What one material costs its bundle.
#[derive(Clone, Copy)]
struct Charge {
    gpu: u64,
    memory: u64,
}

impl Cached {
    /// Charges the material against the `gpu` and `memory` bytes its bundle has left, decoding
    /// its textures only once the charge fits; returns the drawable material and its charge.
    /// A material that no longer fits gives up its decoded textures, so the host never holds
    /// texels its bundle's shares do not cover.
    fn fit(
        &mut self,
        gpu: u64,
        memory: u64,
        textures: &[(String, Vec<u8>)],
    ) -> Result<(&Published, Charge), String> {
        let texels = match &self.outcome {
            Outcome::Refused(reason) => return Err(reason.clone()),
            Outcome::Ready(published) => published.textures.iter().map(Texture::bytes).sum(),
            Outcome::Waiting { texels, .. } => *texels,
        };
        let charge = Charge {
            gpu: texels.saturating_add(PIPELINE_CHARGE_BYTES),
            memory: ((self.spec.source.len() + self.spec.params.len() * 4) as u64)
                .saturating_add(texels),
        };
        let refusal = if charge.gpu > gpu {
            Some("exceeds the bundle's GPU limit")
        } else if charge.memory > memory {
            Some("exceeds the bundle's memory limit")
        } else {
            None
        };
        if let Some(reason) = refusal {
            if let Outcome::Ready(published) = &self.outcome {
                self.outcome = Outcome::Waiting {
                    material: Arc::clone(&published.material),
                    texels,
                };
            }
            return Err(reason.into());
        }
        if let Outcome::Waiting { material, .. } = &self.outcome {
            self.outcome = match decode_all(&self.spec, textures) {
                Ok(decoded) => Outcome::Ready(Published {
                    key: self.attempt,
                    material: Arc::clone(material),
                    textures: decoded.into(),
                    params: params(&self.spec.params),
                    depth: match self.spec.depth {
                        WorldDepth::Test => Depth::Test,
                        WorldDepth::Always => Depth::Always,
                    },
                    blend: match self.spec.blend {
                        WorldBlend::Opaque => Blend::Opaque,
                        WorldBlend::Cutout => Blend::Cutout,
                        WorldBlend::Premultiplied => Blend::Premultiplied,
                        WorldBlend::Additive => Blend::Additive,
                    },
                }),
                Err(reason) => Outcome::Refused(reason),
            };
        }
        match &self.outcome {
            Outcome::Ready(published) => Ok((published, charge)),
            Outcome::Refused(reason) => Err(reason.clone()),
            Outcome::Waiting { .. } => unreachable!("a fitting material was decoded"),
        }
    }
}

/// One slot's owner for the session and the material drawing in it now.
struct Slot {
    bundle: String,
    material: Option<u32>,
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
    /// GPU bytes of each bundle's replaced or removed materials: the renderer cannot destroy
    /// a compiled pipeline, so they stay charged for the session.
    retired: BTreeMap<String, u64>,
    /// Slots by index. A slot belongs to the bundle that first drew in it until the session
    /// ends and only that bundle's materials reuse it, so a mesh still carrying a stale tag
    /// never draws with another bundle's shader.
    slots: Vec<Slot>,
    /// The runtime and world contribution revision the set was last built from.
    built: Option<(u64, u64)>,
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

    /// Rebuilds the set from `live`'s declarations only when they changed since the last
    /// build. Another runtime, or none, ends the previous session first, releasing its
    /// retired charges and slots. Returns whether the set changed.
    pub(super) fn follow<H: super::worker::Worker>(
        &mut self,
        live: Option<&super::live::Live<H>>,
    ) -> bool {
        let built = live.map(super::live::Live::world_contribution);
        if built == self.built {
            return false;
        }
        let mut changed = false;
        if built.map(|(run, _)| run) != self.built.map(|(run, _)| run) {
            changed = self.end_session();
        }
        self.built = built;
        if let Some(live) = live {
            changed |= self.rebuild(live.world_bundles());
        }
        changed
    }

    /// Forgets the session's declarations, charges, slots and diagnostics, publishing an empty
    /// set when the drawn one was not; returns whether it published. Keys and revisions keep
    /// counting, so a renderer never mistakes a new session's material for an old one.
    pub(super) fn end_session(&mut self) -> bool {
        let changed = *self.set
            != WorldMaterialSet {
                revision: self.set.revision,
                ..WorldMaterialSet::default()
            };
        let revision = self.revision + u64::from(changed);
        let set = if changed {
            Arc::new(WorldMaterialSet {
                revision,
                ..WorldMaterialSet::default()
            })
        } else {
            Arc::clone(&self.set)
        };
        *self = Self {
            next_key: self.next_key,
            revision,
            set,
            ..Self::default()
        };
        changed
    }

    /// Rebuilds the set from every bundle's declarations; returns whether it changed. An
    /// unchanged program keeps its compiled material, key and slot.
    pub(super) fn rebuild<'a>(
        &mut self,
        bundles: impl IntoIterator<Item = BundleWorld<'a>>,
    ) -> bool {
        let bundles: Vec<BundleWorld<'a>> = bundles.into_iter().collect();
        let mut drawn: BTreeMap<(String, u32), Published> = BTreeMap::new();
        let mut live = BTreeSet::new();
        for bundle in &bundles {
            let mut used = 0_u64;
            let mut memory = bundle.memory_bytes;
            for (&id, spec) in bundle.world.materials() {
                let key = (bundle.bundle.to_owned(), id);
                live.insert(key.clone());
                let mut cached = match self.cache.remove(&key) {
                    // New parameters keep the compiled shader, its key and its charge; the
                    // renderer writes them into the material's uniform.
                    Some(mut cached) if cached.spec.same_program(spec) => {
                        if cached.spec != *spec {
                            if let Outcome::Ready(published) = &mut cached.outcome {
                                published.params = params(&spec.params);
                            }
                            cached.spec = spec.clone();
                        }
                        cached
                    }
                    previous => {
                        if previous.is_some_and(|cached| cached.published) {
                            self.retire(bundle.bundle);
                        }
                        self.next_key += 1;
                        Cached {
                            spec: spec.clone(),
                            attempt: self.next_key,
                            outcome: measure(spec, bundle.textures),
                            published: false,
                        }
                    }
                };
                // Retirements this rebuild already made count against what the material may use.
                let gpu = bundle
                    .gpu_bytes
                    .saturating_sub(self.retired.get(bundle.bundle).copied().unwrap_or(0))
                    .saturating_sub(used);
                match cached.fit(gpu, memory, bundle.textures) {
                    Ok((published, charge)) => {
                        used += charge.gpu;
                        memory -= charge.memory;
                        drawn.insert(key.clone(), published.clone());
                        cached.published = true;
                    }
                    Err(reason) => self.report(bundle.bundle, id, cached.attempt, &reason),
                }
                self.cache.insert(key, cached);
            }
        }
        let slots = self.assign_slots(&drawn);
        let mut materials: Vec<Option<Published>> = vec![None; self.slots.len()];
        for (key, published) in drawn {
            materials[usize::from(slots[&key])] = Some(published);
        }
        let mut entities = BTreeMap::new();
        let mut blocks = BTreeMap::new();
        for bundle in &bundles {
            for binding in bundle.world.bindings().values() {
                let key = (bundle.bundle.to_owned(), binding.material);
                let Some(&slot) = slots.get(&key) else {
                    continue;
                };
                let (map, identifier, reachable) = match &binding.target {
                    WorldTarget::Entity(identifier) => (
                        &mut entities,
                        identifier,
                        usize::from(slot) < publication::MAX_ENTITY_SLOTS,
                    ),
                    WorldTarget::Block(identifier) => (
                        &mut blocks,
                        identifier,
                        usize::from(slot) < publication::MAX_BLOCK_SLOTS,
                    ),
                };
                // Only the tag width limits which material a target can draw with.
                if !reachable {
                    let attempt = self.cache.get(&key).map_or(0, |cached| cached.attempt);
                    self.report(
                        bundle.bundle,
                        binding.material,
                        attempt,
                        "bound past the session's material slots for its target kind",
                    );
                    continue;
                }
                // Two bundles may sign one namespace; the first in bundle order keeps it.
                map.entry(identifier.as_str().into()).or_insert(slot);
            }
        }
        let removed: Vec<String> = self
            .cache
            .iter()
            .filter(|(key, cached)| !live.contains(*key) && cached.published)
            .map(|((bundle, _), _)| bundle.clone())
            .collect();
        for bundle in removed {
            self.retire(&bundle);
        }
        self.cache.retain(|key, _| live.contains(key));
        if materials == self.set.materials
            && entities == self.set.entities
            && blocks == self.set.blocks
        {
            return false;
        }
        self.revision += 1;
        self.set = Arc::new(WorldMaterialSet {
            revision: self.revision,
            materials,
            entities,
            blocks,
        });
        true
    }

    /// Gives each drawn material a slot: the one it drew in last, else a free slot its bundle
    /// owns, else a new one. Slots past `u16` are left out.
    fn assign_slots(
        &mut self,
        drawn: &BTreeMap<(String, u32), Published>,
    ) -> BTreeMap<(String, u32), u16> {
        let mut assigned = BTreeMap::new();
        for (index, slot) in self.slots.iter_mut().enumerate() {
            let held = slot
                .material
                .map(|id| (slot.bundle.clone(), id))
                .filter(|key| drawn.contains_key(key));
            slot.material = held.as_ref().map(|(_, id)| *id);
            if let (Some(key), Ok(index)) = (held, u16::try_from(index)) {
                assigned.insert(key, index);
            }
        }
        for (bundle, id) in drawn.keys() {
            if assigned.contains_key(&(bundle.clone(), *id)) {
                continue;
            }
            let free = self
                .slots
                .iter()
                .position(|slot| slot.material.is_none() && slot.bundle == *bundle);
            let index = match free {
                Some(index) => index,
                None if u16::try_from(self.slots.len()).is_ok() => {
                    self.slots.push(Slot {
                        bundle: bundle.clone(),
                        material: None,
                    });
                    self.slots.len() - 1
                }
                None => continue,
            };
            self.slots[index].material = Some(*id);
            assigned.insert((bundle.clone(), *id), index as u16);
        }
        assigned
    }

    /// Reports, once each, the published materials whose native pipelines `failed` to compile;
    /// their draws keep the Bedrock material.
    pub(super) fn note_compile_failures(&mut self, failed: impl Fn(u64) -> bool) {
        let failures: Vec<(String, u32, u64)> = self
            .cache
            .iter()
            .filter_map(|((bundle, id), cached)| {
                let Outcome::Ready(published) = &cached.outcome else {
                    return None;
                };
                failed(published.key).then(|| (bundle.clone(), *id, cached.attempt))
            })
            .collect();
        for (bundle, id, attempt) in failures {
            self.report(&bundle, id, attempt, "failed to compile on this GPU");
        }
    }

    /// Charges a replaced or removed material's pipelines to `bundle` for the session.
    fn retire(&mut self, bundle: &str) {
        let retired = self.retired.entry(bundle.to_owned()).or_default();
        *retired = retired.saturating_add(PIPELINE_CHARGE_BYTES);
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
        // Attempts no longer cached cannot recur, so their entries go with them. The attempt
        // being reported may be out of the cache while it is charged, so it stays.
        let cache = &self.cache;
        self.reported
            .retain(|(listed, listed_id, listed_attempt, _)| {
                (listed == bundle && *listed_id == id && *listed_attempt == attempt)
                    || cache
                        .get(&(listed.clone(), *listed_id))
                        .is_some_and(|cached| cached.attempt == *listed_attempt)
            });
    }

    /// Bytes of decoded texels the presenter holds.
    #[cfg(test)]
    fn held_texels(&self) -> u64 {
        self.cache
            .values()
            .filter_map(|cached| match &cached.outcome {
                Outcome::Ready(published) => Some(
                    published
                        .textures
                        .iter()
                        .map(|t| t.rgba.len() as u64)
                        .sum::<u64>(),
                ),
                _ => None,
            })
            .sum()
    }
}

/// Validates the shader again and reads the declared textures' sizes from their headers,
/// decoding nothing; the helper's validation is not trusted.
fn measure(spec: &WorldMaterialSpec, textures: &[(String, Vec<u8>)]) -> Outcome {
    let measured = (|| {
        let material = abi::compose(&spec.source, spec.textures.len())?;
        let mut texels = 0_u64;
        for path in &spec.textures {
            let (width, height) = dimensions(signed(path, textures)?)
                .ok_or_else(|| format!("{path} is not a bounded PNG"))?;
            texels += u64::from(width) * u64::from(height) * 4;
        }
        Ok::<_, String>(Outcome::Waiting {
            material: Arc::new(material),
            texels,
        })
    })();
    measured.unwrap_or_else(Outcome::Refused)
}

/// The signed bytes of the texture at `path`.
fn signed<'a>(path: &str, textures: &'a [(String, Vec<u8>)]) -> Result<&'a [u8], String> {
    textures
        .iter()
        .find(|(listed, _)| listed == path)
        .map(|(_, bytes)| bytes.as_slice())
        .ok_or_else(|| format!("{path} is not a signed texture"))
}

/// Decodes every texture `spec` declares.
fn decode_all(
    spec: &WorldMaterialSpec,
    textures: &[(String, Vec<u8>)],
) -> Result<Vec<Texture>, String> {
    spec.textures
        .iter()
        .map(|path| {
            decode(signed(path, textures)?).ok_or_else(|| format!("{path} is not a bounded PNG"))
        })
        .collect()
}

/// A PNG reader bounded to [`MAX_TEXTURE_EXTENT`] texels a side.
fn reader(bytes: &[u8]) -> image::ImageReader<Cursor<&[u8]>> {
    let mut reader = image::ImageReader::with_format(Cursor::new(bytes), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_TEXTURE_EXTENT);
    limits.max_image_height = Some(MAX_TEXTURE_EXTENT);
    limits.max_alloc = Some(u64::from(MAX_TEXTURE_EXTENT).pow(2) * 8);
    reader.limits(limits);
    reader
}

/// A bounded PNG's size from its header alone.
fn dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    let (width, height) = reader(bytes).into_dimensions().ok()?;
    (width <= MAX_TEXTURE_EXTENT && height <= MAX_TEXTURE_EXTENT).then_some((width, height))
}

/// Decodes a PNG of at most [`MAX_TEXTURE_EXTENT`] texels a side to RGBA8.
fn decode(bytes: &[u8]) -> Option<Texture> {
    let image = reader(bytes).decode().ok()?.to_rgba8();
    Some(Texture {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw().into(),
    })
}

/// A declaration's parameters as the sixteen the renderer uploads, missing ones zero.
fn params(declared: &[f32]) -> [f32; 16] {
    let mut params = [0.0; 16];
    for (slot, value) in params.iter_mut().zip(declared) {
        *slot = *value;
    }
    params
}

#[cfg(test)]
mod tests;
