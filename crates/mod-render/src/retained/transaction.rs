//! Invocation deltas borrow one committed owner; they never fork allocation or retirement state.

use super::*;
use std::{
    collections::BTreeSet,
    sync::{Mutex, Weak},
};

/// The single owner of token allocation and resources awaiting renderer release.
pub(super) struct Owner {
    pub id: u64,
    pub generation: u64,
    pub next_slot: u64,
    pub retired: Vec<Arc<Geometry>>,
    pub retired_materials: Vec<Arc<crate::material::Material>>,
    pub observing: Weak<Mutex<Observations>>,
}

/// Only references visible to the in-flight invocation need confirmed-removal history.
#[derive(Default)]
pub(super) struct Observations {
    pub keys: BTreeSet<EntityKey>,
    pub removed: BTreeSet<EntityKey>,
}

/// Insertions, replacements and explicit removals, relative to the frozen read snapshot.
#[derive(Default)]
pub(super) struct Changes {
    geometry: BTreeMap<Handle, Option<Arc<Geometry>>>,
    materials: BTreeMap<Handle, Option<Arc<crate::material::Material>>>,
    instances: BTreeMap<Handle, Option<Instance>>,
    entities: BTreeMap<Handle, Option<EntityKey>>,
    clips: BTreeMap<Handle, Option<Clip>>,
    groups: BTreeMap<String, Option<Vec<Handle>>>,
}

/// Records changed entries only; unchanged immutable resources remain in the read snapshot.
fn difference<K: Ord + Clone, V: Clone>(
    base: &BTreeMap<K, V>,
    next: &BTreeMap<K, V>,
    same: impl Fn(&V, &V) -> bool,
) -> BTreeMap<K, Option<V>> {
    base.keys()
        .chain(next.keys())
        .filter(|key| match (base.get(*key), next.get(*key)) {
            (Some(base), Some(next)) => !same(base, next),
            (base, next) => base.is_some() != next.is_some(),
        })
        .map(|key| (key.clone(), next.get(key).cloned()))
        .collect()
}

/// Applies explicit changes without replacing newer native entries absent from the delta.
fn apply<K: Ord + Clone, V: Clone>(changes: &BTreeMap<K, Option<V>>, target: &mut BTreeMap<K, V>) {
    for (key, value) in changes {
        match value {
            Some(value) => {
                target.insert(key.clone(), value.clone());
            }
            None => {
                target.remove(key);
            }
        }
    }
}

impl Changes {
    /// Computes the callback's complete delta, with an exhaustive resource-field audit.
    pub(super) fn between(base: &Resources, next: &Resources) -> Self {
        let Resources {
            #[cfg(test)]
                comparison: _,
            connection: _,
            dimension: _,
            geometry,
            materials,
            instances,
            entities,
            clips,
            groups,
            geometry_revision: _,
            material_revision: _,
            instance_revision: _,
        } = next;
        Self {
            // Immutable resources compare by allocation, so placement edits never scan mesh data.
            geometry: difference(&base.geometry, geometry, Arc::ptr_eq),
            materials: difference(&base.materials, materials, Arc::ptr_eq),
            instances: difference(&base.instances, instances, PartialEq::eq),
            entities: difference(&base.entities, entities, PartialEq::eq),
            clips: difference(&base.clips, clips, PartialEq::eq),
            groups: difference(&base.groups, groups, PartialEq::eq),
        }
    }

    /// Merges writes into the current owner and advances only changed resource revisions.
    pub(super) fn apply(&self, target: &mut Resources) {
        let Self {
            geometry,
            materials,
            instances,
            entities,
            clips,
            groups,
        } = self;
        apply(geometry, &mut target.geometry);
        apply(materials, &mut target.materials);
        apply(instances, &mut target.instances);
        apply(entities, &mut target.entities);
        apply(clips, &mut target.clips);
        apply(groups, &mut target.groups);
        target.geometry_revision += u64::from(!geometry.is_empty());
        target.material_revision += u64::from(!materials.is_empty());
        target.instance_revision += u64::from(
            !instances.is_empty()
                || !entities.is_empty()
                || !clips.is_empty()
                || !groups.is_empty(),
        );
    }
}

impl Store {
    /// Freezes reads while sharing the single allocation and retirement owner by reference.
    pub fn transaction(&self) -> Self {
        self.reclaim();
        let observations = Arc::new(Mutex::new(Observations {
            keys: self
                .entity_candidates
                .iter()
                .chain(self.committed.entities.values())
                .copied()
                .collect(),
            removed: BTreeSet::new(),
        }));
        self.owner.lock().expect("drawing owner").observing = Arc::downgrade(&observations);
        Self {
            owner: Arc::clone(&self.owner),
            committed: Arc::clone(&self.committed),
            staged: None,
            mutation_bytes: 0,
            block_mutation_bytes: 0,
            material_compiles: 0,
            entity_candidates: self.entity_candidates.clone(),
            instance_limit: self.instance_limit,
            observations: Some(observations),
        }
    }

    /// Applies a completed invocation's writes to current committed resources, never its owners.
    pub fn accept(&mut self, mut transaction: Self) {
        assert!(Arc::ptr_eq(&self.owner, &transaction.owner));
        let mut resources = (*self.committed).clone();
        let next = transaction.base();
        Changes::between(&transaction.committed, &next).apply(&mut resources);
        if let Some(observations) = &transaction.observations {
            let observations = observations.lock().expect("drawing observations");
            resources
                .entities
                .retain(|_, key| !observations.removed.contains(key));
        }
        reconcile_anchors(&mut resources);
        // Release frozen reads and staged resources before checking retirement reference counts.
        transaction.staged = None;
        drop(next);
        drop(transaction);
        self.install(resources);
    }

    /// Releases renderer-retired allocations once only this committed owner retains them.
    pub fn reclaim(&self) {
        let mut owner = self.owner.lock().expect("drawing owner");
        owner
            .retired
            .retain(|resource| Arc::strong_count(resource) > 1);
        owner
            .retired_materials
            .retain(|resource| Arc::strong_count(resource) > 1);
    }

    /// Keeps immutable allocations removed or replaced by a successful publication alive.
    pub(super) fn install(&mut self, resources: Resources) {
        if resources.revision() == self.committed.revision() {
            self.reclaim();
            return;
        }
        {
            let mut owner = self.owner.lock().expect("drawing owner");
            for (id, geometry) in &self.committed.geometry {
                if resources
                    .geometry
                    .get(id)
                    .is_none_or(|new| !Arc::ptr_eq(new, geometry))
                    && !owner.retired.iter().any(|old| Arc::ptr_eq(old, geometry))
                {
                    owner.retired.push(Arc::clone(geometry));
                }
            }
            for (id, material) in &self.committed.materials {
                if resources
                    .materials
                    .get(id)
                    .is_none_or(|new| !Arc::ptr_eq(new, material))
                    && !owner
                        .retired_materials
                        .iter()
                        .any(|old| Arc::ptr_eq(old, material))
                {
                    owner.retired_materials.push(Arc::clone(material));
                }
            }
        }
        self.committed = Arc::new(resources);
        self.reclaim();
    }

    /// Reconstructs a short-lived validation view from frozen reads and staged deltas.
    pub(super) fn current(&self) -> std::borrow::Cow<'_, Resources> {
        match &self.staged {
            None => std::borrow::Cow::Borrowed(&self.committed),
            Some(changes) => {
                let mut resources = (*self.committed).clone();
                changes.apply(&mut resources);
                std::borrow::Cow::Owned(resources)
            }
        }
    }

    /// Charges the single retirement queue plus replacements awaiting publication.
    pub(super) fn check_budget(&self, candidate: &Resources) -> Result<(), DrawError> {
        let owner = self.owner.lock().expect("drawing owner");
        let mut retired: Vec<_> = owner
            .retired
            .iter()
            .filter(|value| Arc::strong_count(value) > 1)
            .cloned()
            .collect();
        for (id, value) in &self.committed.geometry {
            if candidate
                .geometry
                .get(id)
                .is_none_or(|new| !Arc::ptr_eq(new, value))
                && !retired.iter().any(|old| Arc::ptr_eq(old, value))
            {
                retired.push(Arc::clone(value));
            }
        }
        validation::budget(candidate, &retired)?;
        let retiring_materials = owner
            .retired_materials
            .iter()
            .filter(|value| {
                Arc::strong_count(value) > 1
                    && !candidate
                        .materials
                        .values()
                        .any(|new| Arc::ptr_eq(new, value))
            })
            .count();
        let removed_materials = self
            .committed
            .materials
            .iter()
            .filter(|(id, value)| {
                candidate
                    .materials
                    .get(*id)
                    .is_none_or(|new| !Arc::ptr_eq(new, value))
                    && !owner
                        .retired_materials
                        .iter()
                        .any(|old| Arc::ptr_eq(old, value))
            })
            .count();
        if candidate.materials.len() + retiring_materials + removed_materials
            > mod_api::MAX_DRAW_MATERIALS
        {
            return Err(error(ErrorCode::BudgetExceeded, "materials"));
        }
        Ok(())
    }
}

/// Removes drawings and clips only when their actual entity reference has been removed.
fn reconcile_anchors(resources: &mut Resources) {
    clips::reconcile(resources);
    resources.instances.retain(|_, instance| {
        !matches!(instance.anchor, Anchor::Entity(anchor) if !resources.entities.contains_key(&anchor.entity))
    });
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    /// Makes resource content comparisons fail only in publication regression fixtures.
    #[derive(Clone, Debug, Default)]
    pub(crate) struct ContentComparison(pub bool);

    impl PartialEq for ContentComparison {
        /// Rejects any whole-resource comparison while a fixture is armed.
        fn eq(&self, other: &Self) -> bool {
            assert!(
                !self.0 && !other.0,
                "publication compared resource contents"
            );
            true
        }
    }

    /// Content equality that must never run while building a delta.
    struct Untouchable;
    impl PartialEq for Untouchable {
        fn eq(&self, _: &Self) -> bool {
            panic!("delta compared resource contents")
        }
    }

    #[test]
    fn shared_allocations_are_unchanged_without_comparing_contents() {
        let shared = Arc::new(Untouchable);
        let replaced = Arc::new(Untouchable);
        let base = BTreeMap::from([(1, Arc::clone(&shared)), (2, Arc::clone(&shared))]);
        let next = BTreeMap::from([(1, Arc::clone(&shared)), (2, replaced), (3, shared)]);
        let changes = difference(&base, &next, Arc::ptr_eq);
        assert_eq!(changes.keys().copied().collect::<Vec<_>>(), [2, 3]);
    }

    #[test]
    fn unchanged_publication_never_compares_resource_contents() {
        let mut native = Store::default();
        let actor = EntityKey {
            session: 1,
            dimension: 0,
            runtime_id: 7,
            spawn_revision: 1,
        };
        native.bind_session(actor.session, actor.dimension);
        native.reconcile_entities(vec![actor], &[]).unwrap();
        native.observe_entities().unwrap();
        let geometry = native.create_geometry(super::super::tests::mesh()).unwrap();
        native
            .create_material(crate::material::BUILTIN, DrawingPermissions::ALL)
            .unwrap();
        native.commit();
        Arc::make_mut(&mut native.committed).comparison.0 = true;
        let committed = Arc::clone(native.snapshot());
        native.accept(native.transaction());
        assert!(Arc::ptr_eq(&committed, native.snapshot()));
        native
            .reconcile_entities(
                Vec::new(),
                &[EntityKey {
                    runtime_id: 8,
                    ..actor
                }],
            )
            .unwrap();
        assert!(Arc::ptr_eq(&committed, native.snapshot()));
        native.reconcile_entities(Vec::new(), &[actor]).unwrap();
        assert!(native.snapshot().entities.is_empty());
        assert!(Arc::ptr_eq(
            &committed.geometry[&geometry],
            &native.snapshot().geometry[&geometry]
        ));
    }
}
