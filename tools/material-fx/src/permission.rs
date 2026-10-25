//! `material-fx permission`: withdraws one world material effect permission from the running
//! showcase, or gives it back, through a developer-control client's endpoint, as a live toggle
//! that needs no reconnect.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use developer_control::{client::Controller, protocol::Command};

pub const USAGE: &str =
    "usage: material-fx permission <vertex|scene|passes> <withdraw|restore> [--endpoint <file>]";

/// How long the client has to apply the change and reply.
const REPLY_TIMEOUT: Duration = Duration::from_secs(5);

/// Where a checkout-built client publishes its endpoint when started with
/// `CINNABAR_DEVELOPER_CONTROL` set to it, as the run steps in `world-materials.md` do.
pub fn default_endpoint(root: &Path) -> PathBuf {
    root.join(".local")
        .join(developer_control::ENDPOINT_DIR)
        .join("materialfx.json")
}

/// The command for `effect` (the permission's suffix after `render_world_material_`) and
/// `action`; the client refuses a permission that is not a world material effect.
pub fn command(effect: &str, action: &str) -> Result<Command> {
    let withdrawn = match action {
        "withdraw" => true,
        "restore" => false,
        _ => bail!(USAGE),
    };
    if effect.is_empty() || !effect.bytes().all(|b| b.is_ascii_lowercase()) {
        bail!(USAGE);
    }
    Ok(Command::ExperiencePermission {
        permission: format!("render_world_material_{effect}"),
        withdrawn,
    })
}

/// Parses `args` (after `permission`), sends the command to the client publishing the
/// endpoint, and returns its reply: the permissions now withdrawn.
pub fn run(root: &Path, args: &[String]) -> Result<String> {
    let [effect, action, rest @ ..] = args else {
        bail!(USAGE);
    };
    let endpoint = match rest {
        [] => std::env::var_os(developer_control::ENDPOINT_ENV)
            .map(PathBuf::from)
            .unwrap_or_else(|| default_endpoint(root)),
        [flag, path] if flag == "--endpoint" => PathBuf::from(path),
        _ => bail!(USAGE),
    };
    let command = command(effect, action)?;
    let mut controller = Controller::connect(&endpoint).with_context(|| {
        format!(
            "no client control endpoint at {}; start the client with --features developer-control and {}={}",
            endpoint.display(),
            developer_control::ENDPOINT_ENV,
            endpoint.display()
        )
    })?;
    let reply = controller
        .call(&command, REPLY_TIMEOUT)
        .map_err(anyhow::Error::msg)?;
    Ok(reply.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_effect_names_its_permission_and_bad_arguments_are_refused() {
        let Command::ExperiencePermission {
            permission,
            withdrawn,
        } = command("scene", "withdraw").unwrap()
        else {
            panic!("not a permission command");
        };
        assert_eq!(permission, "render_world_material_scene");
        assert!(withdrawn);
        assert!(matches!(
            command("passes", "restore").unwrap(),
            Command::ExperiencePermission {
                withdrawn: false,
                ..
            }
        ));
        for (effect, action) in [("scene", "toggle"), ("", "withdraw"), ("x y", "restore")] {
            assert!(command(effect, action).is_err(), "{effect:?} {action:?}");
        }
        assert!(run(Path::new("."), &["scene".into()]).is_err());
    }
}
