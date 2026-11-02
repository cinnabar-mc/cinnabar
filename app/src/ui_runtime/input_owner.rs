//! Which screen takes keyboard and pointer input this frame.
//!
//! Screens read input through [`InputOwner`], so a screen gets correct input by being a scene:
//! whatever is on top of the stack as the frame begins owns it. A key that closes the top screen
//! can't also reach the screen beneath it, and a screen opened during the frame reads from the
//! next one.

use bevy::{
    ecs::system::SystemParam,
    prelude::{Res, ResMut},
};
use client_shell::{
    menu::MenuRuntime,
    ui_resources::{FrameInputOwner, UiRuntimeResource},
};
use client_ui::ui_runtime::{
    UiRuntime,
    presentation::forms::scene_policy::MenuScene,
    scene_stack::{Scene, SceneHost},
};

/// The scene on top now, with the menu as this point in the frame sees it.
pub(crate) fn top_scene(
    runtime: &UiRuntime,
    player: &player_state::PlayerState,
    menu: Option<&MenuRuntime>,
) -> Scene {
    let menu = menu.map(|menu| menu as &dyn MenuScene);
    runtime.input_scene(player, SceneHost::of(menu, runtime))
}

/// Decides the frame's owner, before any screen reads input.
pub(crate) fn decide_input_owner(
    player: Res<client_state::PlayerRuntime>,
    runtime: Res<UiRuntimeResource>,
    menu: Option<Res<MenuRuntime>>,
    mut owner: ResMut<FrameInputOwner>,
) {
    owner.0 = top_scene(&runtime, &player, menu.as_deref());
}

/// The frame's input decision, for a system that reads input.
#[derive(SystemParam)]
pub(crate) struct InputOwner<'w> {
    /// Absent only in a harness that runs a screen on its own, where the top scene decides.
    began: Option<ResMut<'w, FrameInputOwner>>,
}

impl InputOwner<'_> {
    /// The decision as it stands at this system's place in the frame.
    pub(crate) fn frame(
        &self,
        runtime: &UiRuntime,
        player: &player_state::PlayerState,
        menu: Option<&MenuRuntime>,
    ) -> InputFrame {
        let top = top_scene(runtime, player, menu);
        InputFrame {
            began: self.began.as_deref().map_or(top, |began| began.0),
            top,
        }
    }

    /// Gives the rest of the frame to `scene`, which took input whatever it leaves on top, as a
    /// screen that opens and closes within one batch of keys does.
    pub(crate) fn claim(&mut self, scene: Scene) {
        if let Some(began) = &mut self.began {
            began.0 = scene;
        }
    }
}

/// The scene that owned input as the frame began, and the scene on top now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct InputFrame {
    began: Scene,
    top: Scene,
}

impl InputFrame {
    /// Whether a system serving the scenes `serves` accepts takes this frame's input: one of them
    /// began the frame on top and one of them still is.
    pub(crate) fn reads(self, serves: impl Fn(Scene) -> bool) -> bool {
        serves(self.began) && serves(self.top)
    }

    /// The scene on top now.
    pub(crate) const fn top(self) -> Scene {
        self.top
    }

    /// Whether a scene above `scene`'s place in the stack is up.
    pub(crate) const fn covers(self, scene: Scene) -> bool {
        self.top.layer() > scene.layer()
    }
}

#[cfg(test)]
mod tests;
