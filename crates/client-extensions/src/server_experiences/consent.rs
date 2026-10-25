//! Host-owned consent status for server experiences.

use server_experience::session::State;

/// Builds plain trusted text; pack data can only fill labeled values. Only an unanswered
/// offer prompts, over whatever is on screen: the join holds its loading screen meanwhile.
pub fn chrome(session: &server_experience::session::Session) -> (Option<String>, bool) {
    let text = match &session.state {
        State::Inert | State::Disabled => return (None, false),
        State::Offered(offer) => {
            let packages = offer
                .offer
                .packages
                .iter()
                .map(|package| {
                    format!(
                        "{} ({} bytes)\nPublisher: {}",
                        package.id, package.bytes, package.publisher_key,
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            let identity = if session.key_changed {
                "Server key changed: new approval required."
            } else {
                "First-use key pinning does not verify the operator's identity."
            };
            let world = world_material_notice(&offer.offer.scope);
            format!(
                concat!(
                    "Server: {}\nKey: {}\n{}\n{}\n",
                    "Permissions: {:?}\nMemory limit: {} bytes; GPU limit: {} bytes\n{}",
                    "{}",
                    "Fallback: {}\nServer code is untrusted. F9 disables it immediately.",
                ),
                offer.offer.audience,
                offer.offer.server_key,
                identity,
                packages,
                offer.offer.scope.permissions,
                offer.offer.scope.memory_bytes,
                offer.offer.scope.gpu_bytes,
                world,
                hosts_notice(&offer.offer.scope.origins),
                offer.offer.fallback,
            )
        }
        State::Awaiting(_) => "Cinnabar: verifying server experience. F9: disable".into(),
        State::Granted(_) => session.notice.clone().unwrap_or_else(|| {
            "Cinnabar: experience approved; runtime unavailable. F9: disable".into()
        }),
    };
    (Some(text), matches!(session.state, State::Offered(_)))
}

/// The consent lines naming the hosts the client may contact for media and downloads. Reserved
/// `.invalid` origins are left out: local servers name them for bundles the client already
/// holds, and the client never contacts them.
fn hosts_notice(origins: &std::collections::BTreeSet<String>) -> String {
    let hosts: Vec<&str> = origins
        .iter()
        .map(String::as_str)
        .filter(|origin| !server_experience::manifest::reserved_origin(origin))
        .collect();
    if hosts.is_empty() {
        "Media/download hosts: none\n".into()
    } else {
        format!(
            "Media/download hosts: {}\nThese hosts see your IP address.\n",
            hosts.join("\n")
        )
    }
}

/// What approving `render_world_material` and each permission extending it allows, one plain
/// line each with the GPU memory that bounds them all, in whole megabytes rounded up; empty
/// when the offer asks for none of them.
pub fn world_material_notice(scope: &server_experience::manifest::Scope) -> String {
    use server_experience::manifest::{Permission, WORLD_MATERIAL_EXTENSIONS};
    let asked = std::iter::once(Permission::RenderWorldMaterial).chain(
        WORLD_MATERIAL_EXTENSIONS
            .iter()
            .map(|extension| extension.permission),
    );
    asked
        .filter(|permission| scope.permissions.contains(permission))
        .filter_map(world_material_line)
        .map(|line| format!("{line} (up to {} MB GPU memory)\n", gpu_megabytes(scope)))
        .collect()
}

/// The consent line of a world material permission, before its GPU memory. The lines of the
/// vertex, scene and passes permissions are provisional wording.
fn world_material_line(
    permission: server_experience::manifest::Permission,
) -> Option<&'static str> {
    use server_experience::manifest::Permission;
    Some(match permission {
        Permission::RenderWorldMaterial => "Change how this server's own mobs and blocks look",
        Permission::RenderWorldMaterialOverride => {
            "Change how any mob or block looks, including vanilla ones"
        }
        Permission::RenderWorldMaterialVertex => {
            "Move and bend how the mobs and blocks it restyles are drawn; where they really are and what you can hit stay the same"
        }
        Permission::RenderWorldMaterialScene => {
            "Let the mobs and blocks it restyles show what is behind them on your screen, bent or blurred"
        }
        Permission::RenderWorldMaterialPasses => {
            "Run extra drawing and GPU work for the mobs and blocks it restyles, such as glows, outlines and particles"
        }
        _ => return None,
    })
}

/// The offer's GPU limit, as the host enforces it, in whole megabytes rounded up.
fn gpu_megabytes(scope: &server_experience::manifest::Scope) -> u64 {
    const MEGABYTE: u64 = 1024 * 1024;
    scope
        .gpu_bytes
        .min(server_experience::policy::MAX_GPU_BYTES)
        .div_ceil(MEGABYTE)
}

/// Leaves all ordinary input untouched when there is no offered experience.
pub fn can_disable(state: &State) -> bool {
    matches!(
        state,
        State::Offered(_) | State::Awaiting(_) | State::Granted(_)
    )
}

#[cfg(test)]
mod status_tests {
    use super::*;

    #[test]
    fn vanilla_session_does_not_claim_disable_key_or_draw_chrome() {
        let session = server_experience::session::Session::default();
        assert!(!can_disable(&session.state));
        assert_eq!(chrome(&session), (None, false));
        assert!(!can_disable(&State::Disabled));
    }

    #[test]
    fn world_material_consent_is_one_plain_line_with_the_gpu_budget() {
        let mut scope = server_experience::manifest::Scope {
            permissions: Default::default(),
            origins: Default::default(),
            memory_bytes: 0,
            gpu_bytes: 4096,
            personal_mods: Default::default(),
        };
        assert!(world_material_notice(&scope).is_empty());
        scope
            .permissions
            .insert(server_experience::manifest::Permission::RenderWorldMaterial);
        assert_eq!(
            world_material_notice(&scope),
            "Change how this server's own mobs and blocks look (up to 1 MB GPU memory)\n"
        );
        scope.gpu_bytes = 32 << 20;
        assert!(world_material_notice(&scope).contains("(up to 32 MB GPU memory)"));
        scope
            .permissions
            .insert(server_experience::manifest::Permission::RenderWorldMaterialOverride);
        assert!(
            world_material_notice(&scope).ends_with(
                "\nChange how any mob or block looks, including vanilla ones (up to 32 MB GPU memory)\n"
            ),
            "the override has its own line"
        );
        // A scope past the host's ceiling is stated at the ceiling the host enforces.
        scope.gpu_bytes = u64::MAX;
        let ceiling = server_experience::policy::MAX_GPU_BYTES >> 20;
        assert!(world_material_notice(&scope).contains(&format!("(up to {ceiling} MB")));
    }

    #[test]
    fn every_world_material_extension_has_its_own_consent_line() {
        use server_experience::manifest::{Permission, WORLD_MATERIAL_EXTENSIONS};
        let mut scope = server_experience::manifest::Scope {
            permissions: [Permission::RenderWorldMaterial].into(),
            origins: Default::default(),
            memory_bytes: 0,
            gpu_bytes: 8 << 20,
            personal_mods: Default::default(),
        };
        let base = world_material_notice(&scope);
        let mut lines = std::collections::BTreeSet::new();
        for extension in WORLD_MATERIAL_EXTENSIONS {
            let line = world_material_line(extension.permission)
                .unwrap_or_else(|| panic!("{} has no consent line", extension.name));
            assert!(lines.insert(line), "{} shares a line", extension.name);
            scope.permissions.insert(extension.permission);
            let notice = world_material_notice(&scope);
            assert!(notice.starts_with(&base));
            assert!(notice.contains(&format!("{line} (up to 8 MB GPU memory)\n")));
            assert!(!line.contains('\n') && line.chars().all(|c| !c.is_control()));
            scope.permissions.remove(&extension.permission);
        }
    }

    #[test]
    fn consent_lists_only_hosts_the_client_may_contact() {
        let origins = |list: &[&str]| list.iter().map(|origin| (*origin).to_owned()).collect();
        assert_eq!(
            hosts_notice(&origins(&["https://cxb.invalid"])),
            "Media/download hosts: none\n"
        );
        assert_eq!(
            hosts_notice(&origins(&[
                "https://cxb.invalid",
                "https://media.example:8443"
            ])),
            "Media/download hosts: https://media.example:8443\nThese hosts see your IP address.\n"
        );
    }
}
