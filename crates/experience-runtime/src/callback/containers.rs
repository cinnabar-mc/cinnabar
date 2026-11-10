//! The vanilla containers in a callback's reach: `container` and `set-container-slot`.

use std::collections::{HashMap, HashSet};

use anyhow::Result;

use super::{CallbackRes, items, stage};
use crate::host::cinnabar::experience_server::types::WorldError;
use crate::limits::{MAX_CONTAINER_DATA_BYTES, MAX_CONTAINER_SLOTS, MAX_CONTAINERS};
use crate::load::Catalog;
use crate::protocol::{BlockPos, Container, Containers, ItemStack, NewStack, Op};

/// The containers a callback may reach, with its staged writes applied.
pub(super) struct Reachable {
    /// The anchor and the members of its network: a container is in reach at one of them or next
    /// to one.
    centres: HashSet<BlockPos>,
    blocks: Vec<Container>,
    /// Each listed position, and each pair's, to its container's index in `blocks`.
    at: HashMap<BlockPos, usize>,
    truncated: bool,
}

impl Reachable {
    /// Checks `containers` as the adapter snapshotted them around `centres`: at most
    /// [`MAX_CONTAINERS`], each in reach and listed once, a pair only beside a loaded chest,
    /// 1 to [`MAX_CONTAINER_SLOTS`] slots each, stacks as an inventory's and at most
    /// [`MAX_CONTAINER_DATA_BYTES`] of item data in all. Anything else is malformed.
    pub(super) fn new(
        own: &Catalog,
        centres: HashSet<BlockPos>,
        containers: Option<&Containers>,
    ) -> Result<Self, String> {
        let Some(containers) = containers else {
            return Ok(Self {
                centres,
                blocks: Vec::new(),
                at: HashMap::new(),
                truncated: false,
            });
        };
        if containers.blocks.len() > MAX_CONTAINERS {
            return Err(format!(
                "{} containers; the limit is {MAX_CONTAINERS}",
                containers.blocks.len()
            ));
        }
        let mut at = HashMap::with_capacity(containers.blocks.len());
        let mut data = 0;
        for (index, container) in containers.blocks.iter().enumerate() {
            let pos = container.pos;
            if !in_reach(&centres, pos) {
                return Err(format!("container {pos:?} is out of reach"));
            }
            for held in std::iter::once(pos).chain(container.pair) {
                if at.insert(held, index).is_some() {
                    return Err(format!("container {held:?} is listed twice"));
                }
            }
            if !container.loaded {
                if container.pair.is_some() || !container.slots.is_empty() {
                    return Err(format!("unloaded container {pos:?} has a pair or slots"));
                }
                continue;
            }
            if let Some(pair) = container.pair
                && !(pair.y == pos.y && neighbours(pos).any(|side| side == pair))
            {
                return Err(format!("the pair of container {pos:?} is not beside it"));
            }
            if container.slots.is_empty() || container.slots.len() > MAX_CONTAINER_SLOTS {
                return Err(format!(
                    "container {pos:?} has {} slots; it may have 1 to {MAX_CONTAINER_SLOTS}",
                    container.slots.len()
                ));
            }
            for (slot, stack) in container.slots.iter().enumerate() {
                if let Some(stack) = stack {
                    let what = format!("slot {slot} of container {pos:?}");
                    data += items::check_stack(own, &what, stack)?;
                }
            }
        }
        if data > MAX_CONTAINER_DATA_BYTES {
            return Err(format!(
                "the containers hold {data} bytes of item data; the limit is \
                 {MAX_CONTAINER_DATA_BYTES}"
            ));
        }
        Ok(Self {
            centres,
            blocks: containers.blocks.clone(),
            at,
            truncated: containers.truncated,
        })
    }

    /// The index of the loaded container that `pos` reaches.
    fn index(&self, pos: BlockPos) -> Result<usize, WorldError> {
        if !in_reach(&self.centres, pos) {
            return Err(WorldError::Denied);
        }
        match self.at.get(&pos) {
            Some(&index) if self.blocks[index].loaded => Ok(index),
            Some(_) => Err(WorldError::Unavailable),
            None if self.truncated => Err(WorldError::Unavailable),
            None => Err(WorldError::UnknownBlock),
        }
    }
}

/// The six positions that share a face with `pos`.
fn neighbours(pos: BlockPos) -> impl Iterator<Item = BlockPos> {
    [
        (1, 0, 0),
        (-1, 0, 0),
        (0, 1, 0),
        (0, -1, 0),
        (0, 0, 1),
        (0, 0, -1),
    ]
    .into_iter()
    .map(move |(x, y, z)| BlockPos {
        x: pos.x + x,
        y: pos.y + y,
        z: pos.z + z,
    })
}

/// Whether `pos` is one of `centres` or next to one.
fn in_reach(centres: &HashSet<BlockPos>, pos: BlockPos) -> bool {
    centres.contains(&pos) || neighbours(pos).any(|side| centres.contains(&side))
}

impl CallbackRes {
    /// The slots of the container that `pos` reaches, with the staged writes applied.
    pub(crate) fn container(
        &mut self,
        pos: BlockPos,
    ) -> Result<Result<Vec<Option<ItemStack>>, WorldError>> {
        self.host_call()?;
        if let Err(error) = self.snapshot.within_height(pos) {
            return Ok(Err(error));
        }
        Ok(self
            .containers
            .index(pos)
            .map(|index| self.containers.blocks[index].slots.clone()))
    }

    /// Stages a slot's new content in the container that `pos` reaches, a stack the guest may
    /// make, or none. The op names the container's listed position, so a slot written again
    /// through either half of a double chest replaces its staged op.
    pub(crate) fn set_container_slot(
        &mut self,
        pos: BlockPos,
        slot: u32,
        stack: Option<NewStack>,
    ) -> Result<Result<(), WorldError>> {
        self.host_call()?;
        if let Err(error) = self.snapshot.within_height(pos) {
            return Ok(Err(error));
        }
        let index = match self.containers.index(pos) {
            Ok(index) => index,
            Err(error) => return Ok(Err(error)),
        };
        let listed = self.containers.blocks[index].pos;
        if slot as usize >= self.containers.blocks[index].slots.len() {
            return Ok(Err(WorldError::OutOfBounds));
        }
        let made = match stack
            .as_ref()
            .map(|stack| items::made(&self.own, stack))
            .transpose()
        {
            Ok(made) => made,
            Err(error) => return Ok(Err(error)),
        };
        let op = Op::SetContainerSlot {
            pos: listed,
            slot,
            stack,
        };
        let earlier = self.ops.iter().position(|op| {
            matches!(op, Op::SetContainerSlot { pos, slot: at, .. } if *pos == listed && *at == slot)
        });
        match earlier {
            Some(earlier) => self.ops[earlier] = op,
            None => stage(&mut self.ops, op)?,
        }
        self.containers.blocks[index].slots[slot as usize] = made;
        Ok(Ok(()))
    }
}
