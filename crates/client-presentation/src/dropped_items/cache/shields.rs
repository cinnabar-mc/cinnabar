//! Retires stack-specific keys and pack owners when their world consumers disappear.
use super::*;

impl ModelCache {
    /// Retries decorated shields at smaller sheet sizes before considering unrelated item art.
    pub(in crate::dropped_items) fn shield_model(
        &mut self,
        mut artwork: client_ui::ui_runtime::presentation::gui_models::WorldShieldArtwork,
    ) -> Option<(ModelKey, u32)> {
        let refusals = self.budget_refusals;
        loop {
            let key = ModelKey::Shield(artwork.clone());
            if let Some(model) = self.model(key.clone(), || artwork.cost(), || artwork.model()) {
                if self.budget_refusals != refusals {
                    self.budget_fallbacks = self.budget_fallbacks.saturating_add(1);
                }
                return Some((key, model));
            }
            artwork = artwork.reduced()?;
        }
    }

    /// Tracks successful and refused shield sources through every model resolution route.
    pub(super) fn observe_shield(&mut self, key: &ModelKey) {
        if matches!(key, ModelKey::Shield(_)) {
            self.shield_demand.insert(key.clone());
        }
    }

    /// Releases unused models and measured keys, so old decorations cannot retain old packs.
    pub(super) fn retire_shields(&mut self) {
        for (slot, resident) in self.residents.iter_mut().enumerate() {
            if resident.as_ref().is_none_or(|entry| {
                !matches!(entry.key, ModelKey::Shield(_)) || self.shield_demand.contains(&entry.key)
            }) {
                continue;
            }
            let entry = resident.take().expect("unused shield resident");
            self.layers -= entry.layers;
            self.pixel_bytes -= pixel_cost(&self.models[slot]);
            self.index.remove(&entry.key);
            self.models[slot] = DroppedItemModel::Vacant;
            self.free_slots.push(slot);
            self.dirty = true;
        }
        let retain = |key: &ModelKey| {
            !matches!(key, ModelKey::Shield(_)) || self.shield_demand.contains(key)
        };
        self.costs.retain(|key, _| retain(key));
        self.rejected.retain(|key, _| retain(key));
        self.invalid_meshes.retain(retain);
        self.budget_rejections.retain(|(key, _, _), _| retain(key));
        #[cfg(test)]
        {
            self.mesh_validations.retain(|key, _| retain(key));
            self.admission_checks.retain(|key, _| retain(key));
        }
    }
}
