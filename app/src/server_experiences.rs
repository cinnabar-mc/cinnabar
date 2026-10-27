//! Trusted session controller; remote data never supplies consent controls.

pub(crate) mod drawing;
mod driver;
mod live;
mod media;
mod worker;
mod world_blocks;
mod world_materials;

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

#[cfg(any(test, feature = "developer-control"))]
/// Withdraws the world material effect permission named `name` from the running client parts
/// for the rest of the session, or gives it back, and returns the names withdrawn now. Only
/// the effect permissions can be withdrawn this way; their effects go in one publication.
pub(crate) fn withdraw_permission(
    world: &mut bevy::prelude::World,
    name: &str,
    withdrawn: bool,
) -> Result<serde_json::Value, String> {
    use server_experience::manifest::{Permission, WORLD_MATERIAL_EXTENSIONS};
    let extension = WORLD_MATERIAL_EXTENSIONS
        .iter()
        .find(|extension| {
            extension.name == name
                && extension.permission != Permission::RenderWorldMaterialOverride
        })
        .ok_or_else(|| format!("{name} is not a world material effect permission"))?;
    let mut withdrawals = world
        .get_resource_mut::<driver::WorldMaterialWithdrawals>()
        .ok_or_else(|| "server experiences are unavailable".to_owned())?;
    if withdrawn {
        withdrawals.permissions.insert(extension.permission);
    } else {
        withdrawals.permissions.remove(&extension.permission);
    }
    let names: Vec<&str> = WORLD_MATERIAL_EXTENSIONS
        .iter()
        .filter(|extension| withdrawals.permissions.contains(&extension.permission))
        .map(|extension| extension.name)
        .collect();
    Ok(serde_json::json!({ "withdrawn": names }))
}

#[cfg(test)]
mod withdrawal_tests {
    use super::*;

    #[test]
    fn only_effect_permissions_are_withdrawn_and_given_back() {
        let mut world = bevy::prelude::World::new();
        world.init_resource::<driver::WorldMaterialWithdrawals>();
        let scene = "render_world_material_scene";
        assert_eq!(
            withdraw_permission(&mut world, scene, true).unwrap(),
            serde_json::json!({ "withdrawn": [scene] })
        );
        withdraw_permission(&mut world, "render_world_material_passes", true).unwrap();
        withdraw_permission(&mut world, scene, false).unwrap();
        assert_eq!(
            world
                .resource::<driver::WorldMaterialWithdrawals>()
                .permissions
                .iter()
                .copied()
                .collect::<Vec<_>>(),
            [server_experience::manifest::Permission::RenderWorldMaterialPasses]
        );
        for refused in [
            "render_world_material_override",
            "render_world_material",
            "ui",
        ] {
            assert!(
                withdraw_permission(&mut world, refused, true).is_err(),
                "{refused}"
            );
        }
    }
}
