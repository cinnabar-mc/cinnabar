//! Numeric admission for HTTP signaling and the same retained NetherNet media owner.

use super::session::PlayLease;
use bridge::{
    ConnectTarget, contract::GATEWAY_EGRESS_CANDIDATES, gateway_egress::public_gateway_upstream,
};
use nethernet_tokio::{
    ConnectionConfig, EgressPolicy, HttpSignaling, NetherClient, SelfSignedIdentity, TokenTrust,
};
use p384::{ecdsa::SigningKey, elliptic_curve::Generate};
use protocol::{NativeAuthentication, NativeRelayConnecting};
use std::{
    io,
    net::SocketAddr,
    sync::Arc,
    time::{Instant, SystemTime},
};
use tokio_rustls::rustls;

#[cfg(test)]
tokio::task_local! {
    pub(super) static FIXTURE_UPSTREAM: SocketAddr;
}

/// Applies the shared direct-server grammar while keeping service routes and URL credentials out.
fn select(target: Option<ConnectTarget>) -> io::Result<(String, Vec<url::Url>)> {
    let Some(ConnectTarget::Server(address)) = target else {
        return Err(refused());
    };
    let address = address.trim().to_owned();
    let endpoints = protocol::native_http_endpoints(&address)
        .map_err(|_| refused())?
        .into_iter()
        .map(|endpoint| url::Url::parse(&endpoint).map_err(|_| refused()))
        .collect::<io::Result<Vec<_>>>()?;
    let first = endpoints.first().ok_or_else(refused)?;
    if endpoints
        .iter()
        .any(|endpoint| endpoint.host() != first.host())
        || first
            .host_str()
            .is_none_or(|host| host.is_empty() || host.contains('%'))
    {
        return Err(refused());
    }
    Ok((address, endpoints))
}

/// Resolves once, pins each admitted HTTP candidate, and retains media egress policy through close.
/// The original proof signs both the SDP offer and subsequent application Login.
pub(super) async fn connect(
    lease: &PlayLease,
    target: Option<ConnectTarget>,
    deadline: Instant,
    authentication: Option<&NativeAuthentication>,
) -> io::Result<NativeRelayConnecting> {
    let (_, endpoints) = select(target)?;
    let deadline = authentication.map_or(deadline, |auth| deadline.min(auth.expires_at()));
    current(lease, deadline)?;
    let first = endpoints.first().ok_or_else(refused)?;
    let host = match first.host().ok_or_else(refused)? {
        url::Host::Domain(host) => host.to_owned(),
        url::Host::Ipv4(address) => address.to_string(),
        url::Host::Ipv6(address) => address.to_string(),
    };
    let port = first.port_or_known_default().ok_or_else(refused)?;
    let resolved = tokio::time::timeout_at(
        deadline.into(),
        tokio::net::lookup_host((host.as_str(), port)),
    )
    .await
    .map_err(|_| elapsed())??;
    current(lease, deadline)?;
    let candidates = resolved.take(GATEWAY_EGRESS_CANDIDATES).collect::<Vec<_>>();
    let (key, mut config) = credentials(authentication, deadline)?;
    config.egress_policy = Some(media_egress());
    let mut failure = refused();
    for endpoint in endpoints {
        let port = endpoint.port_or_known_default().ok_or_else(refused)?;
        let login_address = protocol::native_http_login_address(endpoint.as_str())?;
        for resolved in &candidates {
            current(lease, deadline)?;
            if matches!(resolved, SocketAddr::V6(peer) if peer.scope_id() != 0) {
                continue;
            }
            let candidate = SocketAddr::new(resolved.ip().to_canonical(), port);
            if !admitted(candidate) {
                continue;
            }
            let http = http_client(&host, candidate, deadline)?;
            let signaling = HttpSignaling::with_client(rand::random::<u64>().to_string(), http);
            let mut attempt = config.clone();
            attempt.timeouts.negotiation = attempt
                .timeouts
                .negotiation
                .min(deadline.saturating_duration_since(Instant::now()));
            let connecting = NetherClient::connect_with(signaling, endpoint.to_string(), attempt);
            let connection = match tokio::time::timeout_at(deadline.into(), connecting).await {
                Ok(Ok(connection)) => connection,
                Ok(Err(error)) => {
                    failure = io::Error::other(error);
                    continue;
                }
                Err(_) => return Err(elapsed()),
            };
            let connection =
                NativeRelayConnecting::from_http(connection, login_address, Arc::clone(&key));
            if let Err(error) = current(lease, deadline) {
                let mut retiring = connection.begin_close();
                let _ = retiring.finish_close().await;
                return Err(error);
            }
            return Ok(connection);
        }
    }
    Err(failure)
}

/// Keeps TLS hostname verification while pinning the original admitted socket and disabling reroutes.
pub(super) fn http_client(
    host: &str,
    peer: SocketAddr,
    deadline: Instant,
) -> io::Result<reqwest::Client> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(elapsed());
    }
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::aws_lc_rs::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(io::Error::other)?
    .with_root_certificates(roots)
    .with_no_client_auth();
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .resolve(host, peer)
        .connect_timeout(remaining)
        .timeout(remaining)
        .tls_backend_preconfigured(tls)
        .build()
        .map_err(io::Error::other)
}

/// Retains provider-calibrated SDP authority, or creates one explicit offline proof for this join.
fn credentials(
    authentication: Option<&NativeAuthentication>,
    deadline: Instant,
) -> io::Result<(Arc<SigningKey>, ConnectionConfig)> {
    if let Some(authentication) = authentication {
        return Ok((authentication.signing_key(), authentication.http_config()?));
    }
    let key = Arc::new(
        SigningKey::try_generate_from_rng(&mut rand::rngs::SysRng).map_err(io::Error::other)?,
    );
    let identity = SelfSignedIdentity::from_key(
        Arc::clone(&key),
        SystemTime::now(),
        deadline.saturating_duration_since(Instant::now()),
    )
    .map_err(io::Error::other)?;
    Ok((
        key,
        ConnectionConfig {
            identity: Some(Arc::new(identity)),
            server_trust: Some(TokenTrust::Any),
            attempts: 1,
            ..Default::default()
        },
    ))
}

/// Captures the fixture's exact tuple before actor tasks inherit the admission owner.
fn media_egress() -> EgressPolicy {
    #[cfg(test)]
    let fixture = FIXTURE_UPSTREAM.try_with(|expected| *expected).ok();
    EgressPolicy::new(move |peer| {
        if peer.port() == 0 || matches!(peer, SocketAddr::V6(address) if address.scope_id() != 0) {
            return false;
        }
        let public = public_gateway_upstream(peer.ip());
        #[cfg(test)]
        {
            public || fixture == Some(peer)
        }
        #[cfg(not(test))]
        {
            public
        }
    })
}

/// Checks each final signaling socket; tests may admit only their one exact loopback tuple.
fn admitted(peer: SocketAddr) -> bool {
    if peer.port() == 0 {
        return false;
    }
    let public = public_gateway_upstream(peer.ip());
    #[cfg(test)]
    {
        public
            || FIXTURE_UPSTREAM
                .try_with(|expected| *expected == peer)
                .unwrap_or(false)
    }
    #[cfg(not(test))]
    {
        public
    }
}

/// Rechecks the same join, account, controller and original deadline after caller waits.
pub(super) fn current(lease: &PlayLease, deadline: Instant) -> io::Result<()> {
    if Instant::now() >= deadline {
        return Err(elapsed());
    }
    lease.current()
}

/// Redacts provider, address and resolver details at the application boundary.
fn refused() -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, "gateway upstream refused")
}

/// Keeps expiration distinct from a replaced controller without exposing its identity.
fn elapsed() -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, "gateway join timed out")
}

#[cfg(test)]
mod tests;
