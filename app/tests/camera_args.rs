//! Checks camera arguments against the features enabled in the client library.

use cinnabar::args::{ArgsError, ClientArgs, ParseOutcome};

/// Checks a camera flag against the library's developer and acceptance configuration.
fn assert_camera_flag(flag: &'static str) {
    let parsed = ClientArgs::parse_from(["client", flag]);
    if cfg!(any(feature = "developer-control", feature = "acceptance")) {
        let Ok(ParseOutcome::Run(args)) = parsed else {
            panic!("expected camera arguments, got {parsed:?}");
        };
        assert_eq!(args.freecam, flag == "--freecam");
        assert_eq!(args.auto_fly, flag == "--auto-fly");
    } else {
        assert_eq!(
            parsed,
            Err(ArgsError::DeveloperControlFeatureRequired(flag))
        );
    }
}

#[test]
fn freecam_requires_developer_control_or_acceptance() {
    assert_camera_flag("--freecam");
}

#[test]
fn auto_fly_requires_developer_control_or_acceptance() {
    assert_camera_flag("--auto-fly");
}
