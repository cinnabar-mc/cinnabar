//! Separates network login addresses from local-world display names.

/// Leaves local endpoint selection to the core instead of reporting a world's display name.
pub(super) fn login_server_address(address: &str, local_world: bool) -> String {
    if local_world {
        String::new()
    } else {
        address.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::login_server_address;

    #[test]
    fn local_world_names_leave_login_address_resolution_to_the_core() {
        for name in ["My World", "Survival", "World: One"] {
            assert_eq!(login_server_address(name, true), "");
        }
    }

    #[test]
    fn remote_login_addresses_retain_the_selected_target() {
        for address in [
            "play.example.invalid",
            "play.example.invalid:19133",
            "https://play.example.invalid/v1/join",
        ] {
            assert_eq!(login_server_address(address, false), address);
        }
    }
}
