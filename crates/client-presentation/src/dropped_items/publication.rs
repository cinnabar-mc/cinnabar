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
#[derive(Clone, PartialEq)]
pub(super) struct HeldScene {
    models_revision: u64,
    terrain_session_id: Option<u64>,
    budget: Option<render_model::dropped_item::ItemModelBudget>,
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
    /// `budget` can skip publication. A new budget, such as after a device replacement, is
    /// published.
    pub(super) fn holds(
        &self,
        (revision, budget): (u64, render_model::dropped_item::ItemModelBudget),
        empty: bool,
        scene: &render_scene::DroppedItemScene,
    ) -> Option<HeldScene> {
        let (settled, held, twice) = self.settled.as_ref()?;
        (empty
            && *twice
            && *settled == revision
            && held.budget == Some(budget)
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
        assert!(idle.holds((5, old), true, &scene).is_some());
        assert!(
            idle.holds((5, new), true, &scene).is_none(),
            "a new budget must be published"
        );
        assert!(idle.holds((6, old), true, &scene).is_none());
        assert!(idle.holds((5, old), false, &scene).is_none());
    }
}

#[cfg(test)]
thread_local! {
    /// Empty frames on this test thread that publication could have skipped.
    pub(super) static HELD_FRAMES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}
