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

impl Store {
    /// Revalidates frozen callback references against the latest native observation set.
    /// Already published references survive temporary culling until native code removes them.
    pub fn reconcile_observations(&mut self, latest: &Self) -> Result<(), DrawError> {
        let live: std::collections::BTreeSet<_> = latest
            .entity_candidates
            .iter()
            .chain(latest.committed.entities.values())
            .copied()
            .collect();
        let removed: Vec<_> = self
            .committed
            .entities
            .values()
            .copied()
            .filter(|key| !live.contains(key))
            .collect();
        self.reconcile_entities(latest.entity_candidates.clone(), &removed)
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
        let mut resources = (*self.committed).clone();
        let removed: Vec<_> = resources
            .entities
            .iter()
            .filter_map(|(handle, key)| despawned.contains(key).then_some(*handle))
            .collect();
        resources
            .entities
            .retain(|handle, _| !removed.contains(handle));
        resources.instances.retain(|_, instance| !matches!(instance.anchor, Anchor::Entity(anchor) if removed.contains(&anchor.entity)));
        clips::reconcile(&mut resources);
        if resources != *self.committed {
            resources.instance_revision += 1;
            self.committed = Arc::new(resources);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// One stable actor incarnation used to distinguish culling from confirmed removal.
    fn actor() -> EntityKey {
        EntityKey {
            session: 1,
            dimension: 0,
            runtime_id: 7,
            spawn_revision: 1,
        }
    }

    #[test]
    fn transaction_reconciliation_removes_references_that_departed_while_suspended() {
        let mut native = Store::default();
        native.bind_session(actor().session, actor().dimension);
        native.reconcile_entities(vec![actor()], &[]).unwrap();
        let mut transaction = native.clone();
        transaction.begin();
        transaction.observe_entities().unwrap();
        transaction.commit();
        assert_eq!(transaction.snapshot().entities.len(), 1);
        native.reconcile_entities(Vec::new(), &[actor()]).unwrap();
        transaction.reconcile_observations(&native).unwrap();
        assert!(transaction.snapshot().entities.is_empty());
    }

    #[test]
    fn transaction_reconciliation_keeps_published_references_through_culling() {
        let mut native = Store::default();
        native.bind_session(actor().session, actor().dimension);
        native.reconcile_entities(vec![actor()], &[]).unwrap();
        native.begin();
        let handles = native.observe_entities().unwrap();
        native.commit();
        let mut transaction = native.clone();
        native.reconcile_entities(Vec::new(), &[]).unwrap();
        transaction.reconcile_observations(&native).unwrap();
        assert_eq!(
            transaction.snapshot().entities.get(&handles[0].0),
            Some(&actor())
        );
    }
}
