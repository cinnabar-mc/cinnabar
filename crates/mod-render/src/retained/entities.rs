//! Host-issued actor references; guest tokens never contain a reusable runtime ID alone.
use super::*;

/// Exact actor incarnation in the world authority's namespace.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct EntityKey {
    pub session: u64,
    pub dimension: i32,
    pub runtime_id: u64,
    pub spawn_revision: u64,
}

impl From<mod_api::drawing::MaskEntity> for EntityKey {
    /// Keeps mask and drawing references in the same actor lifetime namespace.
    fn from(key: mod_api::drawing::MaskEntity) -> Self {
        Self {
            session: key.connection,
            dimension: key.dimension,
            runtime_id: key.runtime_id,
            spawn_revision: key.spawn_revision,
        }
    }
}

impl From<EntityKey> for mod_api::drawing::MaskEntity {
    /// Preserves the exact actor incarnation when reconciling mask membership.
    fn from(key: EntityKey) -> Self {
        Self {
            connection: key.session,
            dimension: key.dimension,
            runtime_id: key.runtime_id,
            spawn_revision: key.spawn_revision,
        }
    }
}

impl Store {
    /// Adds other retained actor memberships to this invocation's existing lifetime tracking.
    pub fn track_entities(&self, keys: impl IntoIterator<Item = EntityKey>) {
        if let Some(observations) = &self.observations {
            observations
                .lock()
                .expect("drawing observations")
                .keys
                .extend(keys);
        }
    }

    /// Reads confirmed removals recorded while this invocation was suspended.
    pub fn removed_entities(&self) -> Vec<EntityKey> {
        self.observations
            .as_ref()
            .map_or_else(Vec::new, |observations| {
                observations
                    .lock()
                    .expect("drawing observations")
                    .removed
                    .iter()
                    .copied()
                    .collect()
            })
    }

    /// Includes frozen in-flight observations so native code can report their actual despawns.
    pub fn entity_keys(&self) -> Vec<EntityKey> {
        let mut keys: std::collections::BTreeSet<_> =
            self.committed.entities.values().copied().collect();
        if let Some(observations) = self
            .owner
            .lock()
            .expect("drawing owner")
            .observing
            .upgrade()
        {
            keys.extend(
                observations
                    .lock()
                    .expect("drawing observations")
                    .keys
                    .iter()
                    .copied(),
            );
        }
        keys.into_iter().collect()
    }

    /// Reconciles confirmed despawns immediately; temporary invisibility keeps references.
    pub fn reconcile_entities(
        &mut self,
        mut candidates: Vec<EntityKey>,
        despawned: &[EntityKey],
    ) -> Result<(), DrawError> {
        if candidates.len() > mod_api::MAX_DRAW_ENTITIES {
            return Err(error(ErrorCode::BudgetExceeded, "entity observations"));
        }
        candidates.sort_unstable();
        candidates.dedup();
        self.entity_candidates = candidates;
        if despawned.is_empty() {
            return Ok(());
        }
        if let Some(observations) = self
            .owner
            .lock()
            .expect("drawing owner")
            .observing
            .upgrade()
        {
            let mut observations = observations.lock().expect("drawing observations");
            for key in despawned {
                if observations.keys.contains(key) {
                    observations.removed.insert(*key);
                }
            }
        }
        let mut resources = (*self.committed).clone();
        let removed: Vec<_> = resources
            .entities
            .iter()
            .filter_map(|(handle, key)| despawned.contains(key).then_some(*handle))
            .collect();
        if removed.is_empty() {
            return Ok(());
        }
        resources
            .entities
            .retain(|handle, _| !removed.contains(handle));
        resources.instances.retain(|_, instance| !matches!(instance.anchor, Anchor::Entity(anchor) if removed.contains(&anchor.entity)));
        clips::reconcile(&mut resources);
        resources.instance_revision += 1;
        self.install(resources);
        self.staged = None;
        Ok(())
    }

    /// Issues owner-scoped references to the bounded current observation set atomically.
    pub fn observe_entities(&mut self) -> Result<Vec<(Handle, EntityKey)>, DrawError> {
        let current = self.base();
        let mut result = Vec::new();
        for key in self.entity_candidates.clone() {
            let handle = if let Some((handle, _)) =
                current.entities.iter().find(|(_, known)| **known == key)
            {
                *handle
            } else {
                self.token()?
            };
            result.push((handle, key));
        }
        self.edit(
            result.len() * std::mem::size_of::<EntityKey>(),
            |resources| {
                let previous = resources.entities.len();
                for (handle, key) in &result {
                    resources.entities.insert(*handle, *key);
                }
                if resources.entities.len() > mod_api::MAX_DRAW_ENTITIES {
                    return Err(error(ErrorCode::BudgetExceeded, "entity handles"));
                }
                if resources.entities.len() != previous {
                    resources.instance_revision += 1;
                }
                Ok(())
            },
        )?;
        Ok(result)
    }
}
