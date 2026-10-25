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

/// What approving `render_world_material` allows, as one plain permission line with the GPU
/// memory that bounds it, in whole megabytes rounded up, and a line of its own for
/// `render_world_material_override`; empty when the offer asks for neither.
pub fn world_material_notice(scope: &server_experience::manifest::Scope) -> String {
    use server_experience::manifest::Permission;
    let mut notice = String::new();
    if scope.permissions.contains(&Permission::RenderWorldMaterial) {
        notice.push_str(&format!(
            "Change how this server's own mobs and blocks look (up to {} MB GPU memory)\n",
            gpu_megabytes(scope)
        ));
    }
    if scope
        .permissions
        .contains(&Permission::RenderWorldMaterialOverride)
    {
        notice.push_str(&format!(
            "Change how any mob or block looks, including vanilla ones (up to {} MB GPU memory)\n",
            gpu_megabytes(scope)
        ));
    }
    notice
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
