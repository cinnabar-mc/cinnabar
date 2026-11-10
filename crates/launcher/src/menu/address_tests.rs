//! Saved server addresses preserve IPv6 literals and explicit ports.
use super::*;

#[test]
fn raw_nethernet_targets_are_recognised_in_any_case() {
    use bridge::contract::{NETHERNET_SIGNALING_JSONRPC, NETHERNET_TARGET_PREFIX};
    let target = format!("{NETHERNET_TARGET_PREFIX}{NETHERNET_SIGNALING_JSONRPC}/1234567890");
    assert!(is_nethernet_target(&target));
    assert!(is_nethernet_target(&format!(
        " {} ",
        target.to_ascii_uppercase()
    )));
    for address in ["play.example.net", "nethernet.example.net:19132", "::1", ""] {
        assert!(!is_nethernet_target(address), "{address}");
    }
}

#[test]
fn bare_ipv6_hosts_keep_the_default_port() {
    for host in ["::1", "2001:db8::1234", "::ffff:192.0.2.1"] {
        assert_eq!(split_address(host), (host.into(), DEFAULT_PORT.into()));
        assert_eq!(
            split_address(&format!("[{host}]")),
            (host.into(), DEFAULT_PORT.into())
        );
        assert_eq!(
            split_address(&format!("[{host}]:1234")),
            (host.into(), "1234".into())
        );
    }
}
