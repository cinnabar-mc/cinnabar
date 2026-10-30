use super::*;
use futures::FutureExt;
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

/// A named origin uses its admitted socket and original Host, and a redirect opens no other peer.
#[tokio::test]
async fn signaling_keeps_the_pinned_origin_and_refuses_redirects() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let original = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let denied = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let admitted = original.local_addr().unwrap();
        let redirect = denied.local_addr().unwrap();
        let client = http_client("pinned-origin.invalid", admitted, Instant::now() + Duration::from_secs(5)).unwrap();
        let serving = async {
            let (mut socket, _) = original.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 512];
            while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                let count = socket.read(&mut buffer).await.unwrap();
                assert_ne!(count, 0);
                request.extend_from_slice(&buffer[..count]);
                assert!(request.len() <= 4096, "bounded fixture request header");
            }
            let request = String::from_utf8(request).unwrap().to_ascii_lowercase();
            assert!(request.contains(&format!("host: pinned-origin.invalid:{}\r\n", admitted.port())));
            socket.write_all(format!("HTTP/1.1 302 Found\r\nLocation: http://{redirect}/internal\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
        };
        let requesting = client.get(format!("http://pinned-origin.invalid:{}/v1/join", admitted.port())).send();
        let ((), response) = tokio::join!(serving, requesting);
        assert_eq!(response.unwrap().status(), reqwest::StatusCode::FOUND);
        assert!(denied.accept().now_or_never().is_none(), "redirect opened an unadmitted endpoint");
    }).await.unwrap();
}
