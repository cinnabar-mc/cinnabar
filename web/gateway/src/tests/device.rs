//! Browser device identity through the canonical Hello and actual session owner.
use super::*;
use bedrock_net::login::client_data::RawClientData;
use serde_json::{Value, json};

/// Prepares valid browser metadata without opening a transport or acquiring an account.
fn prepared(session: &crate::session::Session) -> Value {
    let claims = protocol::login_client_data(
        "Fixture",
        protocol::ClientRandomId::new(1).unwrap(),
        None,
        &protocol::LoginSettings::default(),
    );
    let validated = session
        .validate_client_data(
            RawClientData::from_value(&claims).unwrap(),
            protocol::PROTOCOL_VERSION,
        )
        .unwrap();
    serde_json::to_value(validated.into_relay_claims("Fixture").unwrap()).unwrap()
}

#[tokio::test]
async fn headerless_shared_hello_fixtures_supply_browser_device_claims() {
    let contract: Value =
        serde_json::from_str(include_str!("../../../../crates/bridge/data/contract.json")).unwrap();
    let mut checked = 0;
    for fixture in contract["relay"]["fixtures"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|fixture| fixture.get("user_agent").is_some())
    {
        let mut reader = futures::stream::iter([Ok::<_, bridge::BridgeError>(Bytes::from(
            serde_json::to_vec(fixture).unwrap(),
        ))]);
        let hello = control::read_hello(&mut reader, RELAY_CHANNEL_CONTROL)
            .await
            .unwrap();
        assert_eq!(hello.browser_user_agent(""), hello.user_agent);
        assert_eq!(hello.browser_user_agent("Android"), hello.user_agent);
        let registry = Registry::new();
        let attachment = registry
            .open_control(
                Ipv4Addr::LOCALHOST.into(),
                "",
                Instant::now(),
                hello.browser_user_agent(""),
                hello.apple_mobile,
            )
            .unwrap();
        let claims = prepared(&attachment.session);
        assert_eq!(
            claims["DeviceOS"],
            if hello.apple_mobile {
                NATIVE_DEVICE_OS_IOS
            } else {
                NATIVE_DEVICE_OS_WINDOWS
            }
        );
        assert_eq!(claims["DeviceModel"], "");
        assert_eq!(
            claims["DefaultInputMode"],
            if hello.apple_mobile {
                NATIVE_DEVICE_INPUT_TOUCH
            } else {
                NATIVE_DEVICE_INPUT_MOUSE
            }
        );
        checked += 1;
    }
    assert!(checked > 0, "shared browser identity fixture is missing");
}

#[test]
fn browser_device_survives_sign_out_and_controller_resume() {
    let registry = Registry::new();
    let now = Instant::now();
    let address = Ipv4Addr::LOCALHOST.into();
    let first = registry
        .open_control(address, "", now, "Windows NT 10.0", false)
        .unwrap();
    let initial = prepared(&first.session);
    first.session.sign_out(first.generation).unwrap();
    let bearer = first.session.state.lock().unwrap().current.wire();
    let resumed = registry
        .open_control(address, &bearer, now, "Android", false)
        .unwrap();
    let after = prepared(&resumed.session);
    for field in ["DeviceOS", "DeviceModel", "DeviceId", "SelfSignedId"] {
        assert_eq!(
            initial[field], after[field],
            "{field} changed with account/controller state"
        );
    }
    assert_eq!(after["DeviceOS"], NATIVE_DEVICE_OS_WINDOWS);
}

#[test]
fn unknown_browser_retains_its_own_device_and_self_signed_identity() {
    let registry = Registry::new();
    let attachment = registry
        .open_control(
            Ipv4Addr::LOCALHOST.into(),
            "",
            Instant::now(),
            "Unknown browser",
            false,
        )
        .unwrap();
    let mut claims = protocol::login_client_data(
        "Fixture",
        protocol::ClientRandomId::new(1).unwrap(),
        None,
        &protocol::LoginSettings::default(),
    );
    claims["DeviceOS"] = json!(NATIVE_DEVICE_OS_WINDOWS);
    claims["DeviceModel"] = json!("Browser supplied");
    claims["DeviceId"] = json!("00112233445566778899aabbccddeeff");
    claims["SelfSignedId"] = json!("00112233-4455-4677-8899-aabbccddeeff");
    let validated = attachment
        .session
        .validate_client_data(
            RawClientData::from_value(&claims).unwrap(),
            protocol::PROTOCOL_VERSION,
        )
        .unwrap();
    let actual = serde_json::to_value(validated.into_relay_claims("Verified").unwrap()).unwrap();
    for field in ["DeviceOS", "DeviceModel", "DeviceId", "SelfSignedId"] {
        assert_eq!(actual[field], claims[field]);
    }
    assert_eq!(actual["ThirdPartyName"], "Verified");
}
