//! Cryptographic answer policy for destinations returned by authenticated service joins.

use super::*;
use authentication::MessagingTarget;
use bedrock_net::messaging::Recipient;
use nethernet::identity::{
    SdpIdentity, ServerIdentity, TokenIdentity, TokenTrust, validate_sdp, verify_answer_with_policy,
};

const SDP: &str = "v=0\r\na=fingerprint:sha-256 AB:CD\r\nm=application 9\r\n";

/// Uses actual signed SDP to distinguish server-answer authority from player credentials.
#[test]
fn service_answers_accept_server_proofs_without_bypassing_present_identity_checks() {
    let now = UNIX_EPOCH + Duration::from_secs(SERVICE_SECONDS);
    let provider = TokenTrust::Provider(Arc::new(verifier()));
    let server = ServerIdentity::generate("synthetic-server.example", now).unwrap();
    let answer = server.augment(SDP, now).unwrap();
    let provider_answer = TokenIdentity::new(Arc::new(key()), signed(claims(), "synthetic"), now)
        .unwrap()
        .augment(SDP, now)
        .unwrap();
    let untrusted = TokenIdentity::new(Arc::new(key()), signed(claims(), "unknown-key"), now)
        .unwrap()
        .augment(SDP, now)
        .unwrap();
    let mut forged = nethernet::identity::Identity::from_sdp(&answer).unwrap();
    let mut token = Jws::parse(&forged.assertion.token).unwrap();
    token.signature[0] ^= 1;
    let parts: Vec<_> = forged.assertion.token.split('.').collect();
    forged.assertion.token = format!(
        "{}.{}.{}",
        parts[0],
        parts[1],
        URL_SAFE_NO_PAD.encode(token.signature)
    );
    let forged = answer.replace(
        &nethernet::identity::Identity::from_sdp(&answer)
            .unwrap()
            .to_base64()
            .unwrap(),
        &forged.to_base64().unwrap(),
    );

    for (recipient, allow_missing) in [
        (Recipient::Messaging(uuid::Uuid::from_u128(7)), false),
        (Recipient::Legacy("42".into()), true),
    ] {
        let target = MessagingTarget::Service(recipient);
        assert_eq!(target.allow_anonymous_answers(), allow_missing);
        let trust = target.answer_trust(provider.clone());
        let checked =
            verify_answer_with_policy(&answer, Some(&trust), target.allow_anonymous_answers(), now)
                .unwrap()
                .unwrap();
        assert_eq!(
            checked.client_public_key().unwrap(),
            *server.verifying_key()
        );
        assert!(verify_answer_with_policy(&provider_answer, Some(&trust), false, now).is_ok());
        for invalid in [
            answer.replace("AB:CD", "AB:CE"),
            provider_answer.replace("AB:CD", "AB:CE"),
            forged.clone(),
            untrusted.clone(),
            format!("{SDP}a=identity:\r\n"),
        ] {
            assert!(
                verify_answer_with_policy(
                    &invalid,
                    Some(&trust),
                    target.allow_anonymous_answers(),
                    now
                )
                .is_err()
            );
        }
        let absent =
            verify_answer_with_policy(SDP, Some(&trust), target.allow_anonymous_answers(), now);
        if allow_missing {
            assert!(matches!(absent, Ok(None)));
        } else {
            assert!(matches!(
                absent,
                Err(nethernet::identity::error::IdentityError::Missing)
            ));
        }
    }
}

/// A service response does not grant the same server-proof policy to joined friends or raw targets.
#[test]
fn other_messaging_roles_and_accepting_offers_keep_provider_authentication() {
    let now = UNIX_EPOCH + Duration::from_secs(SERVICE_SECONDS);
    let provider = TokenTrust::Provider(Arc::new(verifier()));
    let answer = ServerIdentity::generate("synthetic-server.example", now)
        .unwrap()
        .augment(SDP, now)
        .unwrap();
    let provider_answer = TokenIdentity::new(Arc::new(key()), signed(claims(), "synthetic"), now)
        .unwrap()
        .augment(SDP, now)
        .unwrap();
    let peer = uuid::Uuid::from_u128(7);
    let raw_address = format!(
        "{}{}/{}",
        bridge::contract::NETHERNET_TARGET_PREFIX,
        bridge::contract::NETHERNET_SIGNALING_JSONRPC,
        peer
    );
    let raw = crate::NativeRawTarget::parse(&raw_address)
        .unwrap()
        .unwrap();
    for target in [
        MessagingTarget::Joined(
            crate::NativeMessagingTarget::new(Recipient::Messaging(peer), "original-nonce".into())
                .unwrap(),
        ),
        MessagingTarget::Joined(
            crate::NativeMessagingTarget::new(
                Recipient::Legacy("42".into()),
                "original-nonce".into(),
            )
            .unwrap(),
        ),
        MessagingTarget::Raw(raw),
    ] {
        let trust = target.answer_trust(provider.clone());
        assert!(
            verify_answer_with_policy(&answer, Some(&trust), target.allow_anonymous_answers(), now)
                .is_err()
        );
        assert!(verify_answer_with_policy(&provider_answer, Some(&trust), false, now).is_ok());
    }

    let observed = Instant::now();
    let host = crate::NativeHostingIdentity::new(
        Arc::new(key()),
        credential(
            signed(claims(), "synthetic"),
            observed,
            Duration::from_secs(60),
        ),
        &verifier(),
        XUID,
        observed,
    )
    .unwrap();
    assert!(validate_sdp(&answer, &host.offer_trust(), now).is_err());
    assert!(validate_sdp(&provider_answer, &host.offer_trust(), now).is_ok());
}
