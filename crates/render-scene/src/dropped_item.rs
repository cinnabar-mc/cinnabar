use bevy::{
    prelude::{DetectChangesMut, ResMut, Resource},
    render::extract_resource::ExtractResource,
};

use render_model::{
    DroppedItemInstance, DroppedItemModel, DynamicLightRun, EquipmentRaster, ItemMeshVertex,
    MAX_DROPPED_ITEM_INSTANCES, MAX_DYNAMIC_ITEM_VERTICES, TerrainItemInstance,
};
use std::sync::Arc;
/// The frame's dropped items. `models_revision` must change whenever `models` changes.
#[derive(Clone, Debug, Default, Resource, ExtractResource)]
#[extract_app(bevy::render::RenderApp)]
pub struct DroppedItemScene {
    /// Shared empty budget used to admit every published model before upload.
    pub model_budget: Option<render_model::dropped_item::ItemModelBudget>,
    pub models_revision: u64,
    pub models: Arc<[DroppedItemModel]>,
    pub instances: Arc<[DroppedItemInstance]>,
    pub terrain_session_id: Option<u64>,
    pub terrain_instances: Arc<[TerrainItemInstance]>,
    /// World-space geometry drawn as-is this frame (fishing line, leads).
    pub dynamic: Arc<[ItemMeshVertex]>,
    /// Ascending light runs over `dynamic`; vertices past the last run draw at full brightness.
    pub dynamic_light: Arc<[DynamicLightRun]>,
    pub daylight: f32,
    /// The item glint raster that glinting instances sample.
    pub glint: Option<EquipmentRaster>,
    /// This frame's glint scroll offsets at unit UV scale and strength.
    pub glint_scroll: [f32; 3],
}

impl DroppedItemScene {
    /// Replaces the frame's contents; instances and dynamic vertices beyond their caps are dropped.
    pub fn publish(
        &mut self,
        models_revision: u64,
        models: Arc<[DroppedItemModel]>,
        instances: &[DroppedItemInstance],
        dynamic: &[ItemMeshVertex],
        dynamic_light: &[DynamicLightRun],
        daylight: f32,
    ) {
        if self.models_revision != models_revision {
            self.models_revision = models_revision;
            self.models = models;
        }
        let count = instances.len().min(MAX_DROPPED_ITEM_INSTANCES);
        replace_unless_both_empty(&mut self.instances, &instances[..count]);
        // Whole triangles only.
        let dynamic_count = dynamic.len().min(MAX_DYNAMIC_ITEM_VERTICES) / 3 * 3;
        replace_unless_both_empty(&mut self.dynamic, &dynamic[..dynamic_count]);
        let runs = dynamic_light
            .iter()
            .position(|run| run.end as usize >= dynamic_count)
            .map_or(dynamic_light.len(), |last| last + 1);
        replace_unless_both_empty(&mut self.dynamic_light, &dynamic_light[..runs]);
        self.daylight = if daylight.is_finite() {
            daylight.clamp(0.0, 1.0)
        } else {
            1.0
        };
    }

    /// Supplies the item glint raster and this frame's scroll; a malformed raster is dropped.
    pub fn set_glint(&mut self, raster: Option<EquipmentRaster>, scroll: [f32; 3]) {
        self.glint = raster.filter(|raster| {
            raster.width != 0
                && raster.height != 0
                && raster.rgba8.len() == usize::from(raster.width) * usize::from(raster.height) * 4
        });
        self.glint_scroll = scroll.map(|value| if value.is_finite() { value } else { 0.0 });
    }

    pub fn clear(&mut self) {
        replace_unless_both_empty(&mut self.instances, &[]);
        replace_unless_both_empty(&mut self.dynamic, &[]);
        replace_unless_both_empty(&mut self.dynamic_light, &[]);
        replace_unless_both_empty(&mut self.terrain_instances, &[]);
        self.terrain_session_id = None;
    }

    /// Retains bounded candidates separately; transition lists come from bounded world admission.
    pub fn publish_terrain_instances(
        &mut self,
        session_id: u64,
        instances: &[TerrainItemInstance],
    ) {
        self.terrain_session_id = Some(session_id);
        let count = instances.len().min(MAX_DROPPED_ITEM_INSTANCES);
        replace_unless_both_empty(&mut self.terrain_instances, &instances[..count]);
    }

    /// Whether `self` and `other` publish the same frame to the renderer. Lists compare by
    /// allocation, since every replacement allocates. The glint scroll is compared only while
    /// something is drawn: nothing samples it otherwise, and the frame that draws something again
    /// carries the current scroll.
    #[must_use]
    pub fn publishes_same(&self, other: &Self) -> bool {
        let glint = match (&self.glint, &other.glint) {
            (Some(a), Some(b)) => {
                a.width == b.width && a.height == b.height && Arc::ptr_eq(&a.rgba8, &b.rgba8)
            }
            (None, None) => true,
            _ => false,
        };
        let drawn = !(self.instances.is_empty()
            && self.terrain_instances.is_empty()
            && self.dynamic.is_empty());
        self.model_budget == other.model_budget
            && self.models_revision == other.models_revision
            && Arc::ptr_eq(&self.models, &other.models)
            && Arc::ptr_eq(&self.instances, &other.instances)
            && self.terrain_session_id == other.terrain_session_id
            && Arc::ptr_eq(&self.terrain_instances, &other.terrain_instances)
            && Arc::ptr_eq(&self.dynamic, &other.dynamic)
            && Arc::ptr_eq(&self.dynamic_light, &other.dynamic_light)
            && self.daylight == other.daylight
            && glint
            && (!drawn || self.glint_scroll == other.glint_scroll)
    }

    #[must_use]
    pub fn instance_count(&self) -> usize {
        self.instances.len()
    }
}

/// Lends the dropped-item scene without change detection and, when dropped, marks it changed
/// only if what it publishes differs. The scene is extracted whenever it counts as changed, so a
/// frame with nothing new rewrites nothing.
pub struct DroppedItemRepublish<'a, 'w> {
    scene: &'a mut ResMut<'w, DroppedItemScene>,
    before: DroppedItemScene,
}

impl<'a, 'w> DroppedItemRepublish<'a, 'w> {
    #[must_use]
    pub fn new(scene: &'a mut ResMut<'w, DroppedItemScene>) -> Self {
        let before = DroppedItemScene::clone(scene);
        Self { scene, before }
    }
}

impl std::ops::Deref for DroppedItemRepublish<'_, '_> {
    type Target = DroppedItemScene;
    fn deref(&self) -> &DroppedItemScene {
        self.scene
    }
}

impl std::ops::DerefMut for DroppedItemRepublish<'_, '_> {
    fn deref_mut(&mut self) -> &mut DroppedItemScene {
        self.scene.bypass_change_detection()
    }
}

impl Drop for DroppedItemRepublish<'_, '_> {
    fn drop(&mut self) {
        if !self.scene.publishes_same(&self.before) {
            self.scene.set_changed();
        }
    }
}

/// Publishes `next` into `published`, keeping the existing allocation when both are empty so
/// frames without items allocate nothing.
fn replace_unless_both_empty<T: Clone>(published: &mut Arc<[T]>, next: &[T]) {
    if !(published.is_empty() && next.is_empty()) {
        *published = Arc::from(next);
    }
}

#[cfg(test)]
mod tests;
