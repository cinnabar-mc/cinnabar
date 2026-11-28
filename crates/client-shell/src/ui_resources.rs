//! ECS resources holding the Bevy-free UI authority and its presentation.
use bevy::prelude::{Deref, DerefMut, Resource};
use client_ui::ui_runtime::{UiRuntime, presentation::UiPresentationRuntime, scene_stack::Scene};

/// Shared UI authority for menu, gameplay and network systems.
#[derive(Clone, Debug, Resource, Deref, DerefMut)]
pub struct UiRuntimeResource(pub UiRuntime);

/// The scene on top as this frame's input began: the one that owns the frame's input.
#[derive(Clone, Copy, Debug, Resource)]
pub struct FrameInputOwner(pub Scene);

impl Default for FrameInputOwner {
    fn default() -> Self {
        Self(Scene::Gameplay)
    }
}

/// Retained UI presentation: fonts, icons, screens and the published frame.
#[derive(Resource, Deref, DerefMut)]
pub struct UiPresentationResource(pub UiPresentationRuntime);

/// Applies `mutate` to a resource without marking it changed unless `mutate` reports a change.
///
/// Systems that usually change nothing use this so an idle frame leaves the resource's change
/// tick alone; the UI frame's revision reads the ticks of these resources and the player
/// runtime. `mutate` must return `true` whenever it changed anything a reader can observe.
pub fn update<T: ?Sized>(
    resource: &mut impl bevy::prelude::DetectChangesMut<Inner = T>,
    mutate: impl FnOnce(&mut T) -> bool,
) -> bool {
    let changed = mutate(resource.bypass_change_detection());
    if changed {
        resource.set_changed();
    }
    changed
}
