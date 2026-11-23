//! Read-only spectator streams. A host page names an HTTPS URL whose WebSocket speaks the
//! session endpoint framing directly (Connect, then Handoff and Batch frames) with no relay
//! account or control channel. See `docs/web-spectator.md`.

/// The join identity a spectator stream's handoff may name; such streams never use a separate
/// pack stream, so any nonzero value works.
pub const SPECTATOR_JOIN: u64 = 1;

/// How long the stream's socket may take to open.
pub const OPEN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// How long an open stream may stay silent before its first bytes arrive.
pub const FIRST_BYTES_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// The longest spectator stream URL a page may supply.
pub const MAX_SPECTATOR_URL_BYTES: usize = 2048;

/// Validates a spectator stream URL: HTTPS, or plain HTTP on a loopback host for local
/// development, without credentials or a fragment. Returns its normalized form.
pub fn stream_url(input: &str) -> Result<String, &'static str> {
    if input.len() > MAX_SPECTATOR_URL_BYTES {
        return Err("Spectator stream URL is too long");
    }
    let url = url::Url::parse(input).map_err(|_| "Spectator stream URL is invalid")?;
    let loopback = matches!(
        url.host(),
        Some(url::Host::Domain("localhost"))
            | Some(url::Host::Ipv4(std::net::Ipv4Addr::LOCALHOST))
            | Some(url::Host::Ipv6(std::net::Ipv6Addr::LOCALHOST))
    );
    match url.scheme() {
        "https" => {}
        "http" if loopback => {}
        _ => return Err("Spectator streams require HTTPS"),
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("Spectator stream URLs cannot carry credentials");
    }
    if url.fragment().is_some() {
        return Err("Spectator stream URLs cannot carry a fragment");
    }
    Ok(url.into())
}

/// The address a spectator session reports in its Connect request: the stream's host.
pub fn stream_address(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .unwrap_or_default()
}

/// A byte stream that records whether any bytes have been received, for the first-byte deadline.
pub struct FirstBytes<S> {
    inner: S,
    received: std::rc::Rc<std::cell::Cell<bool>>,
}

impl<S> FirstBytes<S> {
    /// Wraps `inner`; nothing has been received yet.
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            received: std::rc::Rc::default(),
        }
    }

    /// Returns a flag that becomes true once the first bytes are read.
    pub fn received(&self) -> std::rc::Rc<std::cell::Cell<bool>> {
        std::rc::Rc::clone(&self.received)
    }
}

impl<S: tokio::io::AsyncRead + Unpin> tokio::io::AsyncRead for FirstBytes<S> {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        let polled = std::pin::Pin::new(&mut self.inner).poll_read(cx, buf);
        if buf.filled().len() > before {
            self.received.set(true);
        }
        polled
    }
}

impl<S: tokio::io::AsyncWrite + Unpin> tokio::io::AsyncWrite for FirstBytes<S> {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        bytes: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.inner).poll_write(cx, bytes)
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spectator_streams_require_https_except_on_loopback() {
        assert_eq!(
            stream_url("https://example.com/api/spectator/duels/1/stream?camera=pov").as_deref(),
            Ok("https://example.com/api/spectator/duels/1/stream?camera=pov")
        );
        assert!(stream_url("http://localhost:8080/stream").is_ok());
        assert!(stream_url("http://127.0.0.1/stream").is_ok());
        assert!(stream_url("http://[::1]/stream").is_ok());
        assert!(stream_url("http://example.com/stream").is_err());
        assert!(stream_url("wss://example.com/stream").is_err());
        assert!(stream_url("ftp://example.com/stream").is_err());
    }

    #[test]
    fn spectator_stream_urls_reject_credentials_fragments_and_oversized_input() {
        assert!(stream_url("https://user:secret@example.com/stream").is_err());
        assert!(stream_url("https://example.com/stream#viewer").is_err());
        let long = format!(
            "https://example.com/{}",
            "a".repeat(MAX_SPECTATOR_URL_BYTES)
        );
        assert!(stream_url(&long).is_err());
        assert!(stream_url("not a url").is_err());
    }

    #[tokio::test]
    async fn first_bytes_records_only_received_data() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (client, mut server) = tokio::io::duplex(64);
        let mut stream = FirstBytes::new(client);
        let received = stream.received();
        stream.write_all(b"connect").await.unwrap();
        assert!(!received.get());
        server.write_all(b"handoff").await.unwrap();
        let mut buffer = [0; 7];
        stream.read_exact(&mut buffer).await.unwrap();
        assert!(received.get());
    }

    #[test]
    fn spectator_sessions_report_the_stream_host() {
        assert_eq!(
            stream_address("https://example.com:8443/stream"),
            "example.com"
        );
        assert_eq!(stream_address("invalid"), "");
    }
}
