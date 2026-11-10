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
