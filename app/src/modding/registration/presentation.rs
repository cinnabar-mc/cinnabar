//! Retained presentation surfaces for independently registered components.

use super::*;

/// Restores retained surfaces from the remaining hosts after a registration transition.
pub(in crate::modding) fn refresh_presentation(world: &mut World) {
    if !world.contains_resource::<ModRuntime>() {
        return;
    }
    world.resource_scope(|world, mut runtime: Mut<ModRuntime>| {
        let Some(mut presentation) = world.get_resource_mut::<UiPresentationResource>() else {
            return;
        };
        presentation.set_mod_menu_branding(runtime.menu_branding());
        let owner = runtime.hosts.panel_owner();
        let _ = presentation.set_mod_panel_font(runtime.metadata[owner].font.clone());
        let _ = presentation.mod_ui_mut().set_mod_panel(
            runtime.panel_ui_owner(owner),
            runtime.hosts.host(owner).panel(),
        );
        presentation
            .mod_ui_mut()
            .set_mod_panel_open(runtime.hosts.host(owner).panel_open());
        let hidden = runtime.hosts.hidden_hud_elements();
        let mod_ui = presentation.mod_ui_mut();
        mod_ui.set_hud_surfaces(runtime.hud_surfaces());
        mod_ui.set_hidden_hud_elements(hidden);
        presentation.set_mod_presentation(runtime.hosts.merged_presentation());
        let _ = presentation
            .mod_ui_mut()
            .set_mod_label(runtime.hosts.merged_label());
    });
}

/// Releases extension-owned surfaces and gameplay presentation policies.
pub(super) fn clear_presentation(world: &mut World) {
    if let Some(mut style) =
        world.get_resource_mut::<client_presentation::actor_publication::ActorPresentationStyle>()
    {
        *style = Default::default();
    }
    if let Some(mut physics) =
        world.get_resource_mut::<client_interaction::movement::LocalPhysicsController>()
    {
        physics.set_jump_pulse_scope(None);
    }
    if let Some(mut policy) = world.get_resource_mut::<crate::item_use::ModItemUsePolicy>() {
        policy.pulse = false;
    }
    if let Some(mut scene) = world.get_resource_mut::<render_scene::ModRenderScene>() {
        scene.clear();
    }
    if let Some(mut cues) = world.get_resource_mut::<super::super::ModCueFeed>() {
        cues.0.clear();
    }
    if let Some(mut camera) =
        world.get_resource_mut::<client_presentation::camera::CameraSettingsAuthority>()
    {
        camera.set_rig(None);
        camera.set_preserve_teleport_rotation(false);
        camera.set_presented_view(None);
        camera.set_view_scale(1.0, 1.0);
        camera.set_view_tilt(None);
    }
    if let Some(mut presentation) = world.get_resource_mut::<UiPresentationResource>() {
        presentation.set_mod_menu_branding(None);
        presentation.mod_ui_mut().clear_mod_panels();
        presentation
            .mod_ui_mut()
            .set_hud_surfaces(std::iter::empty());
        presentation
            .mod_ui_mut()
            .set_hidden_hud_elements(Default::default());
        let _ = presentation.mod_ui_mut().set_mod_label(None);
        presentation.set_mod_presentation(Default::default());
        if let Err(error) = presentation.set_mod_panel_font(None) {
            bevy::log::warn!("Personal-panel font could not be released: {error}");
        }
    }
}
