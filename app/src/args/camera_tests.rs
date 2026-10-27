use super::{ArgsError, ClientArgs, ParseOutcome};

#[test]
fn camera_flags_reject_without_feature_support() {
    for (flag, args) in [
        (
            "--freecam",
            ClientArgs {
                freecam: true,
                ..ClientArgs::default()
            },
        ),
        (
            "--auto-fly",
            ClientArgs {
                auto_fly: true,
                ..ClientArgs::default()
            },
        ),
    ] {
        assert_eq!(args.validate_camera_support(true), Ok(()));
        assert_eq!(
            args.validate_camera_support(false),
            Err(ArgsError::DeveloperControlFeatureRequired(flag))
        );
    }
    assert_eq!(ClientArgs::default().validate_camera_support(false), Ok(()));
}

#[test]
fn camera_flags_require_developer_control_or_acceptance() {
    for flag in ["--freecam", "--auto-fly"] {
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
}
