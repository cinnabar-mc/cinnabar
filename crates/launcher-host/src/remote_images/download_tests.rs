use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Answers one local HTTP request with the supplied response bytes.
async fn respond(response: &'static [u8]) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/image.png", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = [0; 4096];
        assert!(stream.read(&mut request).await.unwrap() > 0);
        stream.write_all(response).await.unwrap();
    });
    (url, task)
}

#[tokio::test]
async fn rejected_download_reports_the_surface_policy() {
    let client = reqwest::Client::new();
    let failure = download(&client, &FORM_IMAGES, "file:///image.png", false)
        .await
        .unwrap_err();
    assert!(matches!(failure, DownloadFailure::Rejected));
}

#[tokio::test]
async fn refused_status_reports_the_response_status() {
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    for (response, only_ok, status) in [
        (
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".as_slice(),
            false,
            StatusCode::NOT_FOUND,
        ),
        (
            b"HTTP/1.1 206 Partial Content\r\nContent-Length: 0\r\n\r\n".as_slice(),
            true,
            StatusCode::PARTIAL_CONTENT,
        ),
    ] {
        let (url, server) = respond(response).await;
        let failure = download(&client, &FORM_IMAGES, &url, only_ok)
            .await
            .unwrap_err();
        assert!(matches!(failure, DownloadFailure::Status(found) if found == status));
        server.await.unwrap();
    }
}

#[tokio::test]
async fn oversized_download_reports_the_limit_for_declared_and_streamed_bodies() {
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let surface = Surface {
        max_bytes: 4,
        ..FORM_IMAGES
    };
    for response in [
        b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello".as_slice(),
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n".as_slice(),
    ] {
        let (url, server) = respond(response).await;
        let failure = download(&client, &surface, &url, false).await.unwrap_err();
        assert!(matches!(failure, DownloadFailure::TooLarge(limit) if limit == surface.max_bytes));
        server.await.unwrap();
    }
}

#[tokio::test]
async fn request_and_body_failures_preserve_the_underlying_error() {
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    for response in [
        b"invalid HTTP response\r\n\r\n".as_slice(),
        b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nx".as_slice(),
    ] {
        let (url, server) = respond(response).await;
        let failure = download(&client, &FORM_IMAGES, &url, false)
            .await
            .unwrap_err();
        assert!(matches!(failure, DownloadFailure::Request(_)));
        assert!(std::error::Error::source(&failure).is_some());
        server.await.unwrap();
    }
}

#[tokio::test]
async fn form_downloads_still_accept_other_success_statuses() {
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let (url, server) =
        respond(b"HTTP/1.1 206 Partial Content\r\nContent-Length: 2\r\n\r\nok").await;
    assert_eq!(
        download(&client, &FORM_IMAGES, &url, false).await.unwrap(),
        b"ok"
    );
    server.await.unwrap();
}
