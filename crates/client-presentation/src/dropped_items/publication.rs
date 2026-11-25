//! Caches that let item publication skip work whose result is already published.

/// Returns the budget cached for `device`, reading `budget` again whenever `same` finds a
/// different device, so a replacement made while publication was skipped is still noticed.
pub(super) fn cached_device_budget<D: Clone, B: Copy>(
    cache: &mut Option<(D, B)>,
    device: &D,
    same: impl Fn(&D, &D) -> bool,
    budget: impl FnOnce() -> B,
) -> B {
    match cache {
        Some((cached, value)) if same(cached, device) => *value,
        _ => cache.insert((device.clone(), budget())).1,
    }
}

/// What the scene holds after an empty frame, apart from the glint clock that nothing draws.
#[derive(Clone)]
pub(super) struct HeldScene {
    models_revision: u64,
    terrain_session_id: Option<u64>,
    budget: Option<render_model::dropped_item::ItemModelBudget>,
    /// The glint raster the renderer uploads even without instances. Holding its pixels keeps
    /// their address from being reused while the scene compares by it.
    glint: Option<render_model::EquipmentRaster>,
}

impl PartialEq for HeldScene {
    fn eq(&self, other: &Self) -> bool {
        self.models_revision == other.models_revision
            && self.terrain_session_id == other.terrain_session_id
            && self.budget == other.budget
            && same_raster(self.glint.as_ref(), other.glint.as_ref())
    }
}

/// Whether two glint rasters are the same allocation at the same size.
fn same_raster(
    left: Option<&render_model::EquipmentRaster>,
    right: Option<&render_model::EquipmentRaster>,
) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => {
            (left.width, left.height) == (right.width, right.height)
                && std::sync::Arc::ptr_eq(&left.rgba8, &right.rgba8)
        }
        _ => false,
    }
}

impl HeldScene {
    fn of(scene: &render_scene::DroppedItemScene) -> Option<Self> {
        (scene.instances.is_empty()
            && scene.terrain_instances.is_empty()
            && scene.dynamic.is_empty())
        .then_some(Self {
            models_revision: scene.models_revision,
            terrain_session_id: scene.terrain_session_id,
            budget: scene.model_budget,
            glint: scene.glint.clone(),
        })
    }
}

/// Tracks frames with nothing to publish. Once two consecutive empty frames left the model cache
/// at the same revision, a further empty frame at that revision would publish the same empty
/// scene and change no cache state the next frame reads, so it can skip publication. The glint
/// clock it would otherwise advance draws nothing without instances.
#[derive(Default)]
pub(super) struct IdlePublication {
    /// The cache revision and scene of the last empty frame that changed nothing, and whether
    /// the frame before it was one too.
    settled: Option<(u64, HeldScene, bool)>,
}

impl IdlePublication {
    /// The scene to keep when this empty frame at cache `revision` with this frame's model
    /// `budget` and `glint` raster can skip publication. A new budget, such as after a device
    /// replacement, or a new glint raster, such as after a pack reload, is published.
    pub(super) fn holds(
        &self,
        (revision, budget, glint): (
            u64,
            render_model::dropped_item::ItemModelBudget,
            Option<&render_model::EquipmentRaster>,
        ),
        empty: bool,
        scene: &render_scene::DroppedItemScene,
    ) -> Option<HeldScene> {
        let (settled, held, twice) = self.settled.as_ref()?;
        (empty
            && *twice
            && *settled == revision
            && held.budget == Some(budget)
            && same_raster(held.glint.as_ref(), glint)
            && HeldScene::of(scene).as_ref() == Some(held))
        .then(|| held.clone())
    }

    /// Records a fully published frame. Debug and test builds also check that a frame
    /// [`Self::holds`] allowed to skip published exactly the scene it kept.
    pub(super) fn record(
        &mut self,
        (start, end): (u64, u64),
        empty: bool,
        scene: &render_scene::DroppedItemScene,
        held: Option<HeldScene>,
    ) {
        let published = HeldScene::of(scene);
        // Release frames that hold return before publishing, so only checking builds see one.
        #[cfg(not(any(test, debug_assertions)))]
        let _ = held;
        #[cfg(any(test, debug_assertions))]
        assert!(
            held.is_none() || published == held,
            "a skipped empty item frame must leave the scene it would publish"
        );
        #[cfg(test)]
        HELD_FRAMES.with(|frames| frames.set(frames.get() + u64::from(held.is_some())));
        self.settled = match (empty && start == end, published) {
            (true, Some(published)) => {
                let twice = self
                    .settled
                    .as_ref()
                    .is_some_and(|(revision, held, _)| *revision == end && *held == published);
                Some((end, published, twice))
            }
            _ => None,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use render_model::dropped_item::ItemModelBudget;

    /// A settled empty scene is kept only while this frame's model budget matches the one it
    /// published, so a replaced device with new limits republishes.
    #[test]
    fn a_changed_model_budget_republishes_a_settled_empty_scene() {
        let budget = |side| {
            ItemModelBudget::new(
                assets::texture::policy::TexturePolicy::for_device(side),
                256,
                1 << 20,
            )
        };
        let (old, new) = (budget(2048), budget(1024));
        assert_ne!(old, new);
        let scene = render_scene::DroppedItemScene {
            model_budget: Some(old),
            ..Default::default()
        };
        let mut idle = IdlePublication::default();
        for _ in 0..2 {
            idle.record((5, 5), true, &scene, None);
        }
        assert!(idle.holds((5, old, None), true, &scene).is_some());
        assert!(
            idle.holds((5, new, None), true, &scene).is_none(),
            "a new budget must be published"
        );
        assert!(idle.holds((6, old, None), true, &scene).is_none());
        assert!(idle.holds((5, old, None), false, &scene).is_none());
    }

    /// A settled empty scene is kept only while this frame supplies the glint raster it
    /// published, so a pack reload that only replaces the glint uploads it at once.
    #[test]
    fn a_replaced_glint_raster_republishes_a_settled_empty_scene() {
        let raster = || render_model::EquipmentRaster {
            width: 1,
            height: 1,
            rgba8: std::sync::Arc::from([255_u8, 0, 255, 255].as_slice()),
        };
        let (published, reloaded) = (raster(), raster());
        let budget = ItemModelBudget::default();
        let mut scene = render_scene::DroppedItemScene {
            model_budget: Some(budget),
            ..Default::default()
        };
        scene.set_glint(Some(published.clone()), [0.0; 3]);
        let mut idle = IdlePublication::default();
        for _ in 0..2 {
            idle.record((5, 5), true, &scene, None);
        }
        assert!(
            idle.holds((5, budget, Some(&published.clone())), true, &scene)
                .is_some()
        );
        assert!(
            idle.holds((5, budget, Some(&reloaded)), true, &scene)
                .is_none(),
            "equal pixels in a new raster must still be published"
        );
        assert!(idle.holds((5, budget, None), true, &scene).is_none());
    }
}

#[cfg(test)]
thread_local! {
    /// Empty frames on this test thread that publication could have skipped.
    pub(super) static HELD_FRAMES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}
