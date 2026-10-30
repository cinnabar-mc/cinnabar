//! Exact addressed-server selection and the application's verified-key trust decision.

use super::{NativeConnectionTarget, NativeTransport};
use futures::future::BoxFuture;
use p384::ecdsa::VerifyingKey;
use std::{fmt, io, sync::Arc, time::Instant};

/// Application-owned first-use authorization after the actual transport verifies its server key.
/// This decision neither supplies account credentials nor replaces SDP/DTLS authentication.
pub trait NativeServerTrust: Send + Sync {
    /// Decides once under the original join cutoff; cancellation must withdraw this join's prompt.
    fn trust<'a>(
        &'a self,
        endpoint: &'a str,
        public_key: &'a VerifyingKey,
        deadline: Instant,
    ) -> BoxFuture<'a, io::Result<bool>>;
}

#[derive(Clone)]
pub(crate) struct AddressedRoute {
    address: String,
    pub(crate) endpoints: Vec<String>,
    pub(crate) trust: Arc<dyn NativeServerTrust>,
}

impl fmt::Debug for AddressedRoute {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AddressedRoute(..)")
    }
}

impl NativeConnectionTarget {
    /// Binds one addressed endpoint to its original application's trust capability before any I/O.
    ///
    /// # Errors
    /// Rejects empty, malformed or service input; a bare server host may omit its port; explicit HTTP/HTTPS URLs retain their scheme.
    pub fn addressed(address: String, trust: Arc<dyn NativeServerTrust>) -> io::Result<Self> {
        let endpoints = native_http_endpoints(&address)?;
        Ok(Self {
            address: address.clone(),
            transport: NativeTransport::Addressed,
            local_lan: None,
            addressed: Some(AddressedRoute {
                address,
                endpoints,
                trust,
            }),
        })
    }

    /// Rejects changed public projections before credentials, discovery or trust can be used.
    pub(crate) fn addressed_route(&self) -> io::Result<&AddressedRoute> {
        match &self.addressed {
            Some(route)
                if self.transport == NativeTransport::Addressed
                    && self.address == route.address =>
            {
                Ok(route)
            }
            _ => Err(invalid()),
        }
    }
}

/// Produces HTTP signaling candidates without resolving DNS or opening a connection.
/// Explicit HTTP/HTTPS URLs retain their scheme; bare addresses try HTTPS before HTTP.
/// An omitted port tries the Bedrock port and then the scheme's default port.
///
/// # Errors
/// Rejects malformed addresses, service identifiers, credentials, paths and queries.
pub fn native_http_endpoints(address: &str) -> io::Result<Vec<String>> {
    if address.contains("://") {
        let url = url::Url::parse(address).map_err(|_| invalid())?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || !matches!(url.path(), "" | "/")
            || url.query().is_some()
            || url.fragment().is_some()
            || url.port_or_known_default().is_none_or(|port| port == 0)
        {
            return Err(invalid());
        }
        return Ok(vec![url.origin().ascii_serialization()]);
    }
    if address.is_empty()
        || address.contains(['/', '\\', '?', '#', '@'])
        || address
            .chars()
            .any(|value| value.is_whitespace() || value.is_control())
    {
        return Err(invalid());
    }
    let (host, explicit, ipv6) = if let Some((host, port)) = ipv6_authority(address)? {
        (host.to_owned(), port, true)
    } else {
        let url = url::Url::parse(&format!("http://{address}")).map_err(|_| invalid())?;
        (
            url.host().ok_or_else(invalid)?.to_string(),
            address.rsplit_once(':').map(|(_, port)| port),
            false,
        )
    };
    let port = explicit
        .map(str::parse::<u16>)
        .transpose()
        .map_err(|_| invalid())?;
    if port == Some(0) {
        return Err(invalid());
    }
    let authority = if ipv6 { format!("[{host}]") } else { host };
    let mut endpoints = Vec::new();
    for scheme in ["https", "http"] {
        if let Some(port) = port {
            endpoints.push(format!("{scheme}://{authority}:{port}"));
        } else {
            endpoints.push(format!(
                "{scheme}://{authority}:{}",
                bridge::contract::DEFAULT_BEDROCK_PORT
            ));
            endpoints.push(format!("{scheme}://{authority}"));
        }
    }
    Ok(endpoints)
}

/// Formats Login metadata for one selected HTTP origin, retaining its logical host and effective port.
/// This performs no DNS lookup and supplies no socket, server trust or account authority.
///
/// # Errors
/// Rejects input that is not one explicit HTTP/HTTPS origin admitted by [`native_http_endpoints`].
pub fn native_http_login_address(endpoint: &str) -> io::Result<String> {
    let endpoints = native_http_endpoints(endpoint)?;
    let [selected] = endpoints.as_slice() else {
        return Err(invalid());
    };
    let url = url::Url::parse(selected).map_err(|_| invalid())?;
    let port = url.port_or_known_default().ok_or_else(invalid)?;
    let host = match url.host().ok_or_else(invalid)? {
        url::Host::Domain(host) => host.to_owned(),
        url::Host::Ipv4(address) => address.to_string(),
        url::Host::Ipv6(address) => format!("[{address}]"),
    };
    Ok(format!("{}://{host}:{port}", url.scheme()))
}

/// Retains a numeric IPv6 host's optional interface zone independently of HTTP URL syntax.
fn ipv6_authority(address: &str) -> io::Result<Option<(&str, Option<&str>)>> {
    if let Some(bracketed) = address.strip_prefix('[') {
        let (host, suffix) = bracketed.split_once(']').ok_or_else(invalid)?;
        if !ipv6_host(host) {
            return Err(invalid());
        }
        let port = if suffix.is_empty() {
            None
        } else {
            Some(suffix.strip_prefix(':').ok_or_else(invalid)?)
        };
        return Ok(Some((host, port)));
    }
    Ok(ipv6_host(address).then_some((address, None)))
}

/// Validates the numeric address while leaving a nonempty zone for the existing socket resolver.
fn ipv6_host(host: &str) -> bool {
    let (address, valid_zone) = match host.rsplit_once('%') {
        Some((address, zone)) => (address, !zone.is_empty() && !zone.contains(['[', ']'])),
        None => (host, true),
    };
    valid_zone && address.parse::<std::net::Ipv6Addr>().is_ok()
}

/// Keeps endpoint and policy values out of diagnostics.
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "invalid native addressed route",
    )
}
