//! Which slot each drawn material takes. Block quads carry a slot tag in six bits and actor
//! words in eight, so slots below [`publication::MAX_BLOCK_SLOTS`] go to block-bound materials
//! first and entity-only materials take the slots above them. A slot stays its owner's while
//! a mesh may still carry its tag, so a stale tag never draws another bundle's shader.

use mod_render::world_material::publication;
use std::collections::BTreeMap;

/// Who a slot belongs to: a bundle of one runtime.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Owner {
    pub run: u64,
    pub bundle: String,
}

/// One slot's owner and the material drawing in it now.
struct Slot {
    owner: Owner,
    material: Option<u32>,
}

/// A drawn material asking for a slot.
pub(super) struct Request<'a> {
    pub bundle: &'a str,
    pub id: u32,
    /// Whether a block binding names it, so it needs a slot block quads can carry.
    pub blocks: bool,
}

/// The session's slots by index.
#[derive(Default)]
pub(super) struct Slots {
    slots: Vec<Slot>,
}

impl Slots {
    /// Gives each request a slot: the one it drew in last if that still suits it, else a free
    /// slot of the right range its bundle owns, else a free slot no mesh carries (`carried`
    /// holds one bit per block slot), else a new one. A block-bound material takes a block slot
    /// when one is free and an entity slot otherwise, where only its entity bindings reach it;
    /// an entity-only material takes an entity slot first. A request no slot suits is left out.
    pub(super) fn assign(
        &mut self,
        run: u64,
        requests: &[Request<'_>],
        carried: u64,
    ) -> BTreeMap<(String, u32), u16> {
        let owned =
            |slot: &Slot, bundle: &str| slot.owner.run == run && slot.owner.bundle == bundle;
        let mut assigned = BTreeMap::new();
        let mut held = vec![false; self.slots.len()];
        for (index, slot) in self.slots.iter_mut().enumerate() {
            let keep = slot.material.and_then(|id| {
                requests.iter().find(|request| {
                    request.id == id
                        && owned(slot, request.bundle)
                        && (!request.blocks || index < publication::MAX_BLOCK_SLOTS)
                })
            });
            slot.material = keep.map(|request| request.id);
            if let Some(request) = keep {
                held[index] = true;
                assigned.insert((request.bundle.to_owned(), request.id), index as u16);
            }
        }
        let block_range = 0..publication::MAX_BLOCK_SLOTS;
        let entity_range = publication::MAX_BLOCK_SLOTS..publication::MAX_ENTITY_SLOTS;
        let mut order: Vec<&Request<'_>> = requests.iter().collect();
        // Block-bound materials choose first, so entity-only ones cannot use up block slots.
        order.sort_by_key(|request| !request.blocks);
        for request in order {
            let key = (request.bundle.to_owned(), request.id);
            if assigned.contains_key(&key) {
                continue;
            }
            let ranges = if request.blocks {
                [block_range.clone(), entity_range.clone()]
            } else {
                [entity_range.clone(), block_range.clone()]
            };
            let free = |index: usize, slot: &Slot| {
                !held[index]
                    && slot.material.is_none()
                    && (owned(slot, request.bundle)
                        || index >= publication::MAX_BLOCK_SLOTS
                        || carried & (1 << index) == 0)
            };
            let found = ranges.iter().find_map(|range| {
                let mine = range.clone().find(|&index| {
                    self.slots
                        .get(index)
                        .is_some_and(|slot| free(index, slot) && owned(slot, request.bundle))
                });
                let any = || {
                    range
                        .clone()
                        .find(|&index| self.slots.get(index).is_some_and(|slot| free(index, slot)))
                };
                // A range past the table's end opens at its start; the slots skipped stay free.
                let new = || {
                    let next = self.slots.len().max(range.start);
                    (next < range.end).then_some(next)
                };
                mine.or_else(any).or_else(new)
            });
            let Some(index) = found else {
                continue;
            };
            while self.slots.len() <= index {
                self.slots.push(Slot {
                    owner: Owner {
                        run,
                        bundle: request.bundle.to_owned(),
                    },
                    material: None,
                });
                held.push(false);
            }
            let slot = &mut self.slots[index];
            slot.owner = Owner {
                run,
                bundle: request.bundle.to_owned(),
            };
            slot.material = Some(request.id);
            held[index] = true;
            assigned.insert(key, index as u16);
        }
        assigned
    }

    /// Empties every slot without giving it up, as a session ends: each stays reserved for its
    /// owner until no mesh carries its tag.
    pub(super) fn empty(&mut self) {
        for slot in &mut self.slots {
            slot.material = None;
        }
    }

    /// The number of slots the table spans.
    pub(super) fn len(&self) -> usize {
        self.slots.len()
    }
}
