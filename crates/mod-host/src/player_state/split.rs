//! Independently versioned equipment and effect observations, prepared once per callback.

use super::cinnabar::extension::player_state::{Effects, Equipment, Revisions};
use crate::PlayerStateSnapshot;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

#[derive(Default)]
pub(super) struct SplitState {
    pub equipment: Option<Equipment>,
    pub effects: Option<Effects>,
    current: Option<Revisions>,
    observed: bool,
    sequence: Arc<AtomicU64>,
    equipment_revision: u64,
    effects_revision: u64,
}

impl SplitState {
    /// Freezes value comparisons while retaining one allocator for all revision tokens.
    pub(super) fn transaction(&self) -> Self {
        Self {
            equipment: self.equipment.clone(),
            effects: self.effects.clone(),
            current: self.current,
            observed: self.observed,
            sequence: Arc::clone(&self.sequence),
            equipment_revision: self.equipment_revision,
            effects_revision: self.effects_revision,
        }
    }

    /// Issues a unique content token even while native observations advance during suspension.
    fn next_revision(&self) -> anyhow::Result<u64> {
        self.sequence
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map(|previous| previous + 1)
            .map_err(|_| anyhow::anyhow!("player-state revision exhausted"))
    }

    /// Retains exact comparisons while requiring fresh current-session authority next callback.
    pub fn begin_frame(&mut self) {
        self.observed = false;
        self.current = None;
    }

    /// Resolves tokens without copying unchanged inventory strings or vectors.
    pub fn observe(
        &mut self,
        snapshot: Option<&PlayerStateSnapshot>,
    ) -> anyhow::Result<Option<Revisions>> {
        if self.observed {
            return Ok(self.current);
        }
        self.observed = true;
        let Some(s) = snapshot else {
            self.equipment = None;
            self.effects = None;
            return Ok(None);
        };
        if self.equipment.as_ref().is_none_or(|e| {
            e.session != s.session
                || e.dimension != s.dimension
                || e.selected_slot != s.selected_slot
                || e.inventory != s.inventory
                || e.armor != s.armor
                || e.offhand != s.offhand
        }) {
            self.equipment_revision = self.next_revision()?;
            self.equipment = Some(Equipment {
                session: s.session,
                dimension: s.dimension,
                selected_slot: s.selected_slot,
                inventory: s.inventory.clone(),
                armor: s.armor.clone(),
                offhand: s.offhand.clone(),
            });
        }
        if self.effects.as_ref().is_none_or(|e| {
            e.session != s.session || e.dimension != s.dimension || e.effects != s.effects
        }) {
            self.effects_revision = self.next_revision()?;
            self.effects = Some(Effects {
                session: s.session,
                dimension: s.dimension,
                effects: s.effects.clone(),
            });
        }
        self.current = Some(Revisions {
            session: s.session,
            dimension: s.dimension,
            equipment: self.equipment_revision,
            effects: self.effects_revision,
        });
        Ok(self.current)
    }
}
