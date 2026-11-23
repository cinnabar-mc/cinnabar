//! Remembers recent canonical stacks, so stacks drawn every frame skip their digest and tag reads.
use super::{CanonicalItemStack, NetworkItemStack};
use std::sync::{Arc, Mutex};

/// Enough for the hotbar, offhand, armor and held stacks the HUD and hand read each frame.
const CAPACITY: usize = 64;

/// Canonical results by exact stack, for one registry generation. A stack matches only when every
/// field is equal and its tag bytes share the stored allocation, which the memo keeps alive, so
/// a hit returns what canonicalizing the stack again would.
#[derive(Debug, Default)]
pub(super) struct CanonicalMemo(Mutex<Entries>);

#[derive(Debug, Default)]
struct Entries {
    generation: Option<Arc<()>>,
    entries: Vec<(NetworkItemStack, Option<CanonicalItemStack>)>,
    next: usize,
}

impl CanonicalMemo {
    /// Returns `stack`'s canonical form under `generation`, computing it with `canonicalize`
    /// unless an equal stack was canonicalized under the same generation.
    pub(super) fn get(
        &self,
        stack: &NetworkItemStack,
        generation: &Arc<()>,
        canonicalize: impl Fn(&NetworkItemStack) -> Option<CanonicalItemStack>,
    ) -> Option<CanonicalItemStack> {
        let mut memo = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if memo
            .generation
            .as_ref()
            .is_none_or(|current| !Arc::ptr_eq(current, generation))
        {
            memo.generation = Some(Arc::clone(generation));
            memo.entries.clear();
            memo.next = 0;
        }
        let same = |stored: &NetworkItemStack| {
            Arc::ptr_eq(&stored.extra_data, &stack.extra_data)
                && stored.network_id == stack.network_id
                && stored.metadata == stack.metadata
                && stored.stack_network_id == stack.stack_network_id
                && stored.count == stack.count
                && stored.nbt_digest == stack.nbt_digest
                && stored.block_runtime_id == stack.block_runtime_id
        };
        if let Some((_, canonical)) = memo.entries.iter().find(|(stored, _)| same(stored)) {
            let canonical = canonical.clone();
            #[cfg(any(test, debug_assertions))]
            assert_eq!(
                canonical,
                canonicalize(stack),
                "a remembered canonical stack must match a fresh one"
            );
            return canonical;
        }
        let canonical = canonicalize(stack);
        let entry = (stack.clone(), canonical.clone());
        let next = memo.next;
        if memo.entries.len() < CAPACITY {
            memo.entries.push(entry);
        } else {
            memo.entries[next] = entry;
        }
        memo.next = (next + 1) % CAPACITY;
        canonical
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stack(count: u16, extra: &Arc<[u8]>) -> NetworkItemStack {
        use sha2::Digest;
        NetworkItemStack {
            network_id: 1,
            count,
            nbt_digest: sha2::Sha256::digest(extra.as_ref()).into(),
            extra_data: Arc::clone(extra),
            ..NetworkItemStack::empty()
        }
    }

    /// Equal stacks reuse one result; a changed field, new tag bytes or a new registry
    /// generation canonicalize again.
    #[test]
    fn remembers_exact_stacks_per_generation() {
        let memo = CanonicalMemo::default();
        let calls = std::cell::Cell::new(0);
        let canonicalize = |stack: &NetworkItemStack| {
            calls.set(calls.get() + 1);
            Some(CanonicalItemStack {
                identity: assets::ItemStackIdentity {
                    count: stack.count,
                    ..assets::ItemStackIdentity::empty()
                },
                ..crate::item::ItemStateStore::diagnostic()
                    .canonicalize_uncached(&NetworkItemStack::empty())
                    .unwrap()
            })
        };
        let generation = Arc::new(());
        let extra: Arc<[u8]> = Arc::from([1u8, 2, 3]);
        let first = memo.get(&stack(1, &extra), &generation, canonicalize);
        let after_first = calls.get();
        assert_eq!(
            memo.get(&stack(1, &extra), &generation, canonicalize),
            first
        );
        assert_eq!(
            calls.get(),
            after_first + 1,
            "a hit only recomputes for its debug check"
        );
        let counted = memo.get(&stack(2, &extra), &generation, canonicalize);
        assert_eq!(counted.unwrap().identity.count, 2);
        let copied: Arc<[u8]> = Arc::from([1u8, 2, 3]);
        let before = calls.get();
        memo.get(&stack(1, &copied), &generation, canonicalize);
        assert!(calls.get() > before, "new tag bytes canonicalize again");
        let before = calls.get();
        memo.get(&stack(1, &extra), &Arc::new(()), canonicalize);
        assert_eq!(
            calls.get(),
            before + 1,
            "a new generation forgets every stack"
        );
    }
}
