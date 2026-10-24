//! Starts the game client the launcher was installed beside.

use std::{path::Path, process::Command};

use launcher::install_layout::InstallLayout;

/// Starts `layout`'s client as an independent process, joining `address` if one is given; the
/// launcher may exit while it runs.
///
/// The error is a sentence for the failure modal, naming the executable it tried.
pub fn start(layout: &InstallLayout, address: Option<&str>) -> Result<(), String> {
    spawn(&layout.client_executable(), address)
}

fn spawn(executable: &Path, address: Option<&str>) -> Result<(), String> {
    if !executable.is_file() {
        return Err(format!(
            "The game was not found at {}. Reinstall {} to restore it.",
            executable.display(),
            launcher::PRODUCT_NAME
        ));
    }
    let mut command = Command::new(executable);
    if let Some(address) = address {
        command.args([launcher::JOIN_ADDRESS_FLAG, address]);
    }
    command
        .spawn()
        .map(drop)
        .map_err(|error| format!("{} could not start: {error}.", executable.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_client_names_where_it_was_expected() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("cinnabar");
        let message = spawn(&missing, None).unwrap_err();
        assert!(
            message.contains(&missing.display().to_string()),
            "{message}"
        );
    }
}
