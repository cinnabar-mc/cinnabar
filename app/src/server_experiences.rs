//! Trusted session controller; remote data never supplies consent controls.

pub(crate) mod drawing;
mod driver;
mod live;
mod media;
mod worker;

pub(crate) use driver::configure;

/// The client parts' open screens for developer control: each bundle and template, whether it
/// holds focus, and whether the last frame drew it over gameplay.
#[cfg(feature = "developer-control")]
pub(crate) fn developer_screens(world: &bevy::prelude::World) -> Vec<serde_json::Value> {
    let Some(live) = world
        .get_resource::<driver::ExperienceService>()
        .and_then(|service| service.live.as_ref())
    else {
        return Vec::new();
    };
    let shown = world
        .get_resource::<client_shell::ui_resources::UiPresentationResource>()
        .is_some_and(|presentation| presentation.mod_ui().experience_modal_shown());
    live.open_screens()
        .into_iter()
        .map(|(bundle, template, focused)| {
            serde_json::json!({
                "bundle": bundle,
                "template": template,
                "focused": focused,
                "drawn": focused && shown,
            })
        })
        .collect()
}
