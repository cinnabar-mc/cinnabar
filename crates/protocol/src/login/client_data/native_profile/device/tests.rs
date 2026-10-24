use super::*;
use serde_json::json;

#[test]
fn migration_refreshes_native_claims_without_replacing_compatible_install_ids() {
    let id = "00112233445566778899aabbccddeeff";
    let saved = serde_json::to_vec(
        &json!({"os":NATIVE_DEVICE_OS_ANDROID,"model":"Old fabricated phone","id":id}),
    )
    .unwrap();
    for os in [
        NATIVE_DEVICE_OS_MACOS,
        NATIVE_DEVICE_OS_WINDOWS,
        NATIVE_DEVICE_OS_LINUX,
    ] {
        let fresh = NativeDeviceProfile::new(os, "Actual hardware");
        let current = fresh.migrate(Some(&saved));
        assert_eq!(current.os, os);
        assert_eq!(current.model, "Actual hardware");
        assert_eq!(current.id, id);
        let serialized = serde_json::to_vec(&current).unwrap();
        assert_eq!(fresh.migrate(Some(&serialized)).id, id);
    }
    let mut ios_host = NativeDeviceProfile::new(NATIVE_DEVICE_OS_IOS, "iPhone");
    ios_host.id = "00112233445566778899AABBCCDDEEFF".into();
    let ios = ios_host.migrate(Some(&saved));
    assert_ne!(ios.id, id);
    assert!(compatible_id(NATIVE_DEVICE_OS_IOS, &ios.id));
    let ios_saved = serde_json::to_vec(&ios).unwrap();
    assert_eq!(
        NativeDeviceProfile::new(NATIVE_DEVICE_OS_IOS, "")
            .migrate(Some(&ios_saved))
            .id,
        ios.id
    );
    let fresh = NativeDeviceProfile::new(NATIVE_DEVICE_OS_LINUX, "");
    assert_eq!(fresh.migrate(Some(b"not JSON")).id, fresh.id);
    assert_eq!(fresh.migrate(Some(br#"{"id":"invalid"}"#)).id, fresh.id);
}

#[test]
fn serialized_platform_and_input_claims_replace_old_mobile_desktop_values() {
    for (os, platform, input) in [
        (
            NATIVE_DEVICE_OS_MACOS,
            NATIVE_DEVICE_PLATFORM_DESKTOP,
            NATIVE_DEVICE_INPUT_MOUSE,
        ),
        (
            NATIVE_DEVICE_OS_WINDOWS,
            NATIVE_DEVICE_PLATFORM_DESKTOP,
            NATIVE_DEVICE_INPUT_MOUSE,
        ),
        (
            NATIVE_DEVICE_OS_LINUX,
            NATIVE_DEVICE_PLATFORM_DESKTOP,
            NATIVE_DEVICE_INPUT_MOUSE,
        ),
        (
            NATIVE_DEVICE_OS_IOS,
            NATIVE_DEVICE_PLATFORM_MOBILE,
            NATIVE_DEVICE_INPUT_TOUCH,
        ),
        (
            NATIVE_DEVICE_OS_ANDROID,
            NATIVE_DEVICE_PLATFORM_MOBILE,
            NATIVE_DEVICE_INPUT_TOUCH,
        ),
    ] {
        let profile = NativeDeviceProfile::new(os, "Detected hardware");
        let mut claims = json!({"PlatformType":NATIVE_DEVICE_PLATFORM_MOBILE});
        profile
            .apply(
                &mut claims,
                crate::ClientRandomId::new(1).unwrap(),
                "Fixture",
            )
            .unwrap();
        let encoded = serde_json::to_vec(&claims).unwrap();
        let claims: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(claims["DeviceOS"], os);
        assert_eq!(claims["DeviceModel"], "Detected hardware");
        assert_eq!(claims["PlatformType"], platform);
        assert_eq!(claims["DefaultInputMode"], input);
    }
    let android = NativeDeviceProfile::new(NATIVE_DEVICE_OS_ANDROID, "");
    let mut explicit = json!({"PlatformType": 3});
    android
        .apply(
            &mut explicit,
            crate::ClientRandomId::new(1).unwrap(),
            "Fixture",
        )
        .unwrap();
    assert_eq!(explicit["PlatformType"], 3);
}

#[test]
fn browser_identity_recognizes_native_platforms_without_a_relay_hardware_model() {
    for (agent, mobile, os) in [
        ("Linux; Android 15", false, NATIVE_DEVICE_OS_ANDROID),
        ("iPhone; CPU iPhone OS", false, NATIVE_DEVICE_OS_IOS),
        ("iPad; CPU OS", false, NATIVE_DEVICE_OS_IOS),
        ("iPod", false, NATIVE_DEVICE_OS_IOS),
        ("Windows NT 10.0", false, NATIVE_DEVICE_OS_WINDOWS),
        ("Macintosh; Intel Mac OS X", false, NATIVE_DEVICE_OS_MACOS),
        ("Macintosh; Intel Mac OS X", true, NATIVE_DEVICE_OS_IOS),
        ("X11; Linux x86_64", false, NATIVE_DEVICE_OS_LINUX),
        ("X11; CrOS", false, NATIVE_DEVICE_OS_LINUX),
    ] {
        let profile = NativeDeviceProfile::browser(agent, mobile).unwrap();
        assert_eq!(profile.os, os);
        assert!(profile.model.is_empty());
        assert_eq!(profile.id.len(), 32);
        assert!(profile.id.bytes().all(|byte| byte.is_ascii_hexdigit()));
        if os == NATIVE_DEVICE_OS_IOS {
            assert_eq!(profile.id, profile.id.to_ascii_uppercase());
        } else {
            assert!(compatible_id(os, &profile.id));
        }
    }
    assert!(NativeDeviceProfile::browser("unknown browser", false).is_none());
    for (target, os) in [
        ("android", NATIVE_DEVICE_OS_ANDROID),
        ("ios", NATIVE_DEVICE_OS_IOS),
        ("macos", NATIVE_DEVICE_OS_MACOS),
        ("windows", NATIVE_DEVICE_OS_WINDOWS),
        ("linux", NATIVE_DEVICE_OS_LINUX),
        ("other", NATIVE_DEVICE_OS_UNKNOWN),
    ] {
        assert_eq!(host_os(target), os);
    }
}
