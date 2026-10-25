//! Hands the blocks bound to world materials to terrain meshing as network-value tags.

use crate::runtime::world::ClientWorld;
use bevy::prelude::{Local, Res, ResMut};
use meshing::WorldMaterialBlock;
use mod_render::world_material::publication::{Blend, WorldMaterialSet};
use render_scene::{ChunkRenderQueue, world_material::WorldMaterialScene};
use std::sync::Arc;

/// The tags last resolved, keyed by session, block-name revision and set revision.
type Resolved = ((u64, u64, u64), Arc<[WorldMaterialBlock]>);
/// The stream session and its world material progress when a change was last held.
type Held = Option<(u64, u64)>;

/// Resolves every bound block identifier to its network values and tags them with one more
/// than their material slot, marking those whose material lets the world behind show. The
/// stream remeshes only the sections holding a block whose entry changed, and the sections
/// beside them when it changed whether the block hides faces; their uploads still queued go
/// too, so no mesh tagged before the change reaches the renderer after it and binding,
/// unbinding and revocation show without a reconnect. A change waits while the previous one
/// still remeshes. An unchanged frame costs a key comparison and a pointer comparison.
pub(super) fn tag_world_material_blocks(
    scene: Option<Res<WorldMaterialScene>>,
    mut client_world: ResMut<ClientWorld>,
    (mut resolved, mut held): (Local<Option<Resolved>>, Local<Held>),
    queue: Option<ResMut<ChunkRenderQueue>>,
) {
    let Some(stream) = client_world.stream.as_ref() else {
        return;
    };
    let authority = stream.authority();
    let set = scene.as_deref().map(|scene| &scene.set);
    let key = (
        authority.actor_session_id(),
        authority.block_name_revision(),
        set.map_or(0, |set| set.revision),
    );
    let tags = match &*resolved {
        Some((resolved_key, tags)) if *resolved_key == key => Arc::clone(tags),
        _ => {
            let tags = tags(set.map(|set| &**set), |name| {
                authority.block_ids_for_name(name)
            });
            *resolved = Some((key, Arc::clone(&tags)));
            tags
        }
    };
    if Arc::ptr_eq(stream.world_material_tags(), &tags) {
        return;
    }
    // A held change retries only once this stream reports progress on the outstanding one.
    let session = authority.actor_session_id();
    if still_held(*held, session, stream.world_material_progress()) {
        return;
    }
    let Some(stream) = client_world.stream.as_mut() else {
        return;
    };
    // While the previous change still remeshes, this one waits for a later frame.
    let Some(withdrawn) = stream.set_world_materials(tags, web_time::Instant::now()) else {
        *held = Some((session, stream.world_material_progress()));
        return;
    };
    *held = None;
    if let Some(mut queue) = queue {
        queue.withdraw_uploads(withdrawn);
    }
}

/// Whether a change `held` (the stream session and its progress when it was held) is still
/// waiting: only while the same stream has made no progress, so a replacement stream's first
/// change is never skipped.
fn still_held(held: Held, session: u64, progress: u64) -> bool {
    held == Some((session, progress))
}

/// The bound blocks of `set` by sorted unique network value, each with one more than its
/// slot and whether its material is anything but opaque; a slot past the quad tag's range is
/// left untagged.
fn tags<'a>(
    set: Option<&WorldMaterialSet>,
    ids: impl Fn(&str) -> &'a [u32],
) -> Arc<[WorldMaterialBlock]> {
    let mut tags: Vec<WorldMaterialBlock> = set
        .into_iter()
        .flat_map(|set| {
            set.blocks
                .iter()
                .map(move |(name, slot)| (set, name, *slot))
        })
        .filter_map(|(set, name, slot)| {
            let tag = u8::try_from(slot + 1)
                .ok()
                .filter(|tag| *tag <= meshing::PackedQuad::MAX_WORLD_MATERIAL)?;
            let see_through = set
                .materials
                .get(usize::from(slot))
                .and_then(Option::as_ref)
                .is_some_and(|material| material.blend != Blend::Opaque);
            Some(
                ids(name)
                    .iter()
                    .map(move |id| WorldMaterialBlock::new(*id, tag, see_through)),
            )
        })
        .flatten()
        .collect();
    tags.sort_unstable();
    tags.dedup_by_key(|block| block.network_value);
    tags.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mod_render::world_material::{
        WorldMaterial,
        publication::{Depth, Published},
    };

    /// A set whose slots draw with `blends` in order and bind `blocks` to them.
    fn set(blends: &[Blend], blocks: &[(&str, u16)]) -> WorldMaterialSet {
        let material = Arc::new(WorldMaterial {
            source: "".into(),
            textures: 0,
            features: Default::default(),
        });
        WorldMaterialSet {
            revision: 1,
            materials: blends
                .iter()
                .enumerate()
                .map(|(slot, blend)| {
                    Some(Published {
                        key: slot as u64 + 1,
                        material: Arc::clone(&material),
                        textures: Arc::from([]),
                        depth: Depth::Test,
                        blend: *blend,
                        effects: Default::default(),
                    })
                })
                .collect(),
            entities: Default::default(),
            blocks: blocks
                .iter()
                .map(|(name, slot)| ((*name).into(), *slot))
                .collect(),
            computes: Vec::new(),
        }
    }

    #[test]
    fn a_held_change_waits_only_for_progress_on_its_own_stream() {
        assert!(!still_held(None, 1, 0));
        assert!(still_held(Some((1, 0)), 1, 0));
        assert!(!still_held(Some((1, 0)), 1, 1), "the stream made progress");
        assert!(
            !still_held(Some((1, 3)), 2, 3),
            "a replacement stream at the same progress is not held"
        );
    }

    #[test]
    fn bound_blocks_tag_every_network_value_once() {
        let set = set(
            &[Blend::Opaque, Blend::Opaque, Blend::Opaque],
            &[("example:lamp", 0), ("example:post", 2)],
        );
        let ids = |name: &str| -> &'static [u32] {
            match name {
                "example:lamp" => &[40, 7],
                "example:post" => &[9],
                _ => &[],
            }
        };
        assert_eq!(
            &*tags(Some(&set), ids),
            &[
                WorldMaterialBlock::new(7, 1, false),
                WorldMaterialBlock::new(9, 3, false),
                WorldMaterialBlock::new(40, 1, false),
            ]
        );
        assert!(tags(None, ids).is_empty());
    }

    #[test]
    fn blocks_with_cutout_or_blended_materials_are_see_through() {
        let set = set(
            &[
                Blend::Opaque,
                Blend::Cutout,
                Blend::Premultiplied,
                Blend::Additive,
            ],
            &[("a:stone", 0), ("a:leaf", 1), ("a:glass", 2), ("a:glow", 3)],
        );
        let ids = |name: &str| -> &'static [u32] {
            match name {
                "a:stone" => &[1],
                "a:leaf" => &[2],
                "a:glass" => &[3],
                _ => &[4],
            }
        };
        let see_through: Vec<bool> = tags(Some(&set), ids)
            .iter()
            .map(|block| block.see_through)
            .collect();
        assert_eq!(see_through, [false, true, true, true]);
    }
}
