//! Shared session generation, transfer addressing and off-frame retirement.

use client_state::PlayerRuntime;
use client_ui::ui_runtime::UiRuntime;

/// Moves UI and player authority to `generation` together; repeating it is a no-op.
pub fn begin_session(ui: &mut UiRuntime, player: &mut PlayerRuntime, generation: u64) {
    if ui.session_id() != generation {
        player.begin_session(generation);
        ui.begin_session(generation);
    }
}

/// Renders a validated transfer host and port as a dialable address.
///
/// IPv6 literals are bracketed the way the Go core's dialer expects.
pub fn format_transfer_address(host: &str, port: u16) -> String {
    if host.starts_with('[') && host.ends_with(']') {
        format!("{host}:{port}")
    } else if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

/// The address a server-directed transfer dials, or `None` for an unusable host.
///
/// Well-formedness only, like the protocol boundary: vanilla servers
/// legitimately transfer across unrelated hosts, so there is no allowlist.
pub fn transfer_handoff_address(host: &str, port: u16) -> Option<String> {
    let trimmed = host.trim();
    (!trimmed.is_empty()).then(|| {
        if port == 0 {
            format!(
                "{}{}/{}",
                bridge::contract::NETHERNET_TARGET_PREFIX,
                bridge::contract::NETHERNET_SIGNALING_JSONRPC,
                trimmed
            )
        } else {
            format_transfer_address(trimmed, port)
        }
    })
}

/// Retains a loopback endpoint for identity transfers from a local proxy.
pub fn loopback_login_address(address: &str) -> Option<&str> {
    let literal = address
        .parse::<std::net::SocketAddr>()
        .is_ok_and(|addr| addr.ip().is_loopback() && addr.port() != 0);
    let localhost = address.rsplit_once(':').is_some_and(|(host, port)| {
        host.eq_ignore_ascii_case("localhost") && port.parse::<u16>().is_ok_and(|port| port != 0)
    });
    (literal || localhost).then_some(address)
}

/// Drops retired world and pack snapshots on a worker to avoid frame-thread destruction.
/// Falls back to dropping here only when the worker cannot start.
#[cfg(not(target_arch = "wasm32"))]
pub fn release_off_frame(retired: impl Send + 'static) {
    let spawned = std::thread::Builder::new()
        .name("session-release".to_owned())
        .spawn(move || {
            let _span = bevy::log::info_span!("session.release_allocations").entered();
            drop(retired);
        });
    if let Err(error) = spawned {
        bevy::log::warn!("session release thread unavailable, released on the frame: {error}");
    }
}

/// Retires snapshots through the browser's bounded event-loop driver.
#[cfg(target_arch = "wasm32")]
pub fn release_off_frame(retired: impl Send + 'static) {
    crate::browser_jobs::retire(move || drop(retired));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transfer_addresses_bracket_ipv6_and_leave_ordinary_hosts_untouched() {
        assert_eq!(
            format_transfer_address("game.example.net", 19133),
            "game.example.net:19133"
        );
        assert_eq!(format_transfer_address("::1", 19132), "[::1]:19132");
        assert_eq!(
            format_transfer_address("2001:db8::10", 25565),
            "[2001:db8::10]:25565"
        );
        assert_eq!(
            format_transfer_address("[2001:db8::10]", 25565),
            "[2001:db8::10]:25565",
            "an already-bracketed transfer literal must not be bracketed twice",
        );
    }

    #[test]
    fn handoff_targets_are_well_formed_without_any_host_allowlist() {
        assert_eq!(
            transfer_handoff_address(" game.example.net ", 19133).as_deref(),
            Some("game.example.net:19133"),
            "a trimmed well-formed host is a valid target"
        );
        assert_eq!(
            transfer_handoff_address("minigames.other-host.example", 19321).as_deref(),
            Some("minigames.other-host.example:19321"),
            "cross-host transfers are legitimate vanilla behavior"
        );
        assert!(transfer_handoff_address("", 19132).is_none());
        assert!(transfer_handoff_address("   ", 19132).is_none());
        assert_eq!(
            transfer_handoff_address("123456789", 0).as_deref(),
            Some(
                format!(
                    "{}{}/123456789",
                    bridge::contract::NETHERNET_TARGET_PREFIX,
                    bridge::contract::NETHERNET_SIGNALING_JSONRPC
                )
                .as_str()
            )
        );
    }

    #[test]
    fn local_proxy_login_address_preserves_only_loopback_endpoints() {
        for address in ["127.0.0.1:19132", "[::1]:19133", "localhost:19132"] {
            assert_eq!(loopback_login_address(address), Some(address));
        }
        for address in [
            "example.com:19132",
            "192.0.2.1:19132",
            "localhost:0",
            "nethernet/jsonrpc/id",
        ] {
            assert_eq!(loopback_login_address(address), None);
        }
    }

    /// A retired world is freed on another thread, never by the frame that leaves the server.
    #[test]
    fn a_retired_session_is_released_off_the_calling_thread() {
        struct Probe(std::sync::mpsc::Sender<std::thread::ThreadId>);
        impl Drop for Probe {
            fn drop(&mut self) {
                let _ = self.0.send(std::thread::current().id());
            }
        }
        let (dropped, dropper) = std::sync::mpsc::channel();
        release_off_frame((Probe(dropped), vec![0_u8; 1024]));
        let thread = dropper
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("the retired session is released");
        assert_ne!(thread, std::thread::current().id());
    }
}
