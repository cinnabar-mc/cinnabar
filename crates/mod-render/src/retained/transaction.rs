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
fn difference<K: Ord + Clone, V: Clone + PartialEq>(
    base: &BTreeMap<K, V>,
    next: &BTreeMap<K, V>,
) -> BTreeMap<K, Option<V>> {
    base.keys()
        .chain(next.keys())
        .filter_map(|key| {
            (base.get(key) != next.get(key)).then(|| (key.clone(), next.get(key).cloned()))
        })
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
            geometry: difference(&base.geometry, geometry),
            materials: difference(&base.materials, materials),
            instances: difference(&base.instances, instances),
            entities: difference(&base.entities, entities),
            clips: difference(&base.clips, clips),
            groups: difference(&base.groups, groups),
        }
    }

    /// Merges writes into the current owner and advances only changed resource revisions.
    pub(super) fn apply(&self, target: &mut Resources) {
        apply(&self.geometry, &mut target.geometry);
        apply(&self.materials, &mut target.materials);
        apply(&self.instances, &mut target.instances);
        apply(&self.entities, &mut target.entities);
        apply(&self.clips, &mut target.clips);
        apply(&self.groups, &mut target.groups);
        target.geometry_revision += u64::from(!self.geometry.is_empty());
        target.material_revision += u64::from(!self.materials.is_empty());
        target.instance_revision += u64::from(
            !self.instances.is_empty()
                || !self.entities.is_empty()
                || !self.clips.is_empty()
                || !self.groups.is_empty(),
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
        if resources == *self.committed {
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
                    .get(id)
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
