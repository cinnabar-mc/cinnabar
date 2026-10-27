//! Hands the blocks bound to world materials to terrain meshing as network-value tags.

use crate::runtime::world::ClientWorld;
use bevy::prelude::{Local, Res, ResMut};
use render_scene::{ChunkRenderQueue, world_material::WorldMaterialScene};
use std::sync::Arc;

/// The tags last resolved, keyed by session, block-name revision and set revision.
type Resolved = ((u64, u64, u64), Arc<[(u32, u8)]>);

/// Resolves every bound block identifier to its network values and tags them with one more
/// than their material slot; a change remeshes, so binding, unbinding and revocation show
/// without a reconnect.
pub(super) fn tag_world_material_blocks(
    scene: Option<Res<WorldMaterialScene>>,
    mut client_world: ResMut<ClientWorld>,
    mut resolved: Local<Option<Resolved>>,
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
            let tags = tags(set.map(|set| &set.blocks), |name| {
                authority.block_ids_for_name(name)
            });
            *resolved = Some((key, Arc::clone(&tags)));
            tags
        }
    };
    if stream.world_materials() != &*tags
        && let Some(stream) = client_world.stream.as_mut()
    {
        stream.set_world_materials(tags, web_time::Instant::now());
        if let Some(mut queue) = queue {
            queue.discard_resource_work();
        }
    }
}

/// Sorted unique network values of the bound blocks, each with one more than its slot; a
/// slot past the quad tag's range is left untagged.
fn tags<'a>(
    blocks: Option<&std::collections::BTreeMap<Box<str>, u16>>,
    ids: impl Fn(&str) -> &'a [u32],
) -> Arc<[(u32, u8)]> {
    let mut tags: Vec<(u32, u8)> = blocks
        .into_iter()
        .flatten()
        .filter_map(|(name, slot)| {
            let tag = u8::try_from(slot + 1)
                .ok()
                .filter(|tag| *tag <= meshing::PackedQuad::MAX_WORLD_MATERIAL)?;
            Some(ids(name).iter().map(move |id| (*id, tag)))
        })
        .flatten()
        .collect();
    tags.sort_unstable();
    tags.dedup_by_key(|(id, _)| *id);
    tags.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bound_blocks_tag_every_network_value_once() {
        let blocks = std::collections::BTreeMap::from([
            ("example:lamp".into(), 0_u16),
            ("example:post".into(), 2),
        ]);
        let ids = |name: &str| -> &'static [u32] {
            match name {
                "example:lamp" => &[40, 7],
                "example:post" => &[9],
                _ => &[],
            }
        };
        assert_eq!(&*tags(Some(&blocks), ids), &[(7, 1), (9, 3), (40, 1)]);
        assert!(tags(None, ids).is_empty());
    }
}
