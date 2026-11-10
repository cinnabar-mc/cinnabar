use super::{ArgsError, ClientArgs};

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
