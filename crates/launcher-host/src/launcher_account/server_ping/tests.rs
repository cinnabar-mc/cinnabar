use super::*;
use nethernet_tokio::ServerData;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

/// Reads the real status request without accepting unbounded fixture input.
async fn request(peer: &mut TcpStream) {
    let mut bytes = Vec::new();
    while !bytes.ends_with(b"\r\n\r\n") {
        assert!(
            bytes.len() < 8192,
            "status request exceeded the fixture budget"
        );
        bytes.push(peer.read_u8().await.unwrap());
    }
    assert_eq!(
        std::str::from_utf8(&bytes).unwrap().lines().next().unwrap(),
        format!(
            "GET {} HTTP/1.1",
            nethernet::signaling::http::join::STATUS_PATH
        )
    );
}

/// Writes a complete anonymous status response and closes its original HTTP connection.
async fn respond(listener: &TcpListener, status: &str, body: &str) {
    let (mut peer, _) = listener.accept().await.unwrap();
    request(&mut peer).await;
    peer.write_all(
        format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .as_bytes(),
    )
    .await
    .unwrap();
}

/// A later row can finish first without changing row identity, status text or count policy.
#[tokio::test]
async fn native_row_ping_reads_http_status_and_preserves_input_order() {
    let first = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let second = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addresses = vec![
        format!("http://{}", first.local_addr().unwrap()),
        format!("http://{}", second.local_addr().unwrap()),
        format!("{}fixture", bridge::contract::GATHERING_TARGET_PREFIX),
    ];
    let mut data = ServerData::new("  Native\\ Fixture  ".into(), "World".into());
    data.player_count = 12;
    data.max_player_count = -1;
    let body = data.to_json();
    let second_body = ServerData::new("Second".into(), "World".into()).to_json();
    let (release, released) = tokio::sync::oneshot::channel();
    let answer_first = async {
        released.await.unwrap();
        respond(&first, "200 OK", &body).await;
    };
    let answer_second = async {
        respond(&second, "200 OK", &second_body).await;
        release.send(()).unwrap();
    };
    let (results, (), ()) = tokio::join!(round(&addresses), answer_first, answer_second);
    assert_eq!(
        results.iter().map(|row| &row.address).collect::<Vec<_>>(),
        addresses.iter().collect::<Vec<_>>()
    );
    assert!(results[0].online && results[1].online);
    assert_eq!(results[0].motd, "Native\\ Fixture");
    assert_eq!(results[0].players, 12);
    assert_eq!(results[0].max_players, 0);
    assert_eq!(results[1].motd, "Second");
    assert!(!results[2].online);
}

/// Explicit IPv6 HTTP targets use their original family rather than a reconstructed UDP route.
#[tokio::test]
async fn native_row_ping_reads_ipv6_status_when_available() {
    let listener = match TcpListener::bind("[::1]:0").await {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!(
                "missing-fixture: IPv6 loopback HTTP unavailable ({})",
                error.kind()
            );
            return;
        }
    };
    let address = format!("http://{}", listener.local_addr().unwrap());
    let body = ServerData::new("IPv6".into(), "World".into()).to_json();
    let (result, ()) = tokio::join!(one(&address), respond(&listener, "200 OK", &body));
    assert!(result.online);
    assert_eq!(result.address, address);
    assert_eq!(result.motd, "IPv6");
}

/// A failed response or malformed body never publishes an online row.
#[tokio::test]
async fn native_row_ping_rejects_failed_and_malformed_http_status() {
    let valid = ServerData::new("Fixture".into(), "World".into()).to_json();
    for (status, body) in [
        ("503 Service Unavailable", valid.as_str()),
        ("200 OK", "not json"),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let (result, ()) = tokio::join!(one(&address), respond(&listener, status, body));
        assert!(!result.online);
        assert_eq!(result.address, address);
        assert!(result.motd.is_empty());
        assert_eq!(result.players, 0);
    }
}

/// The original whole-attempt cutoff includes a status body that stops after its headers.
#[tokio::test]
async fn native_row_ping_times_out_the_original_incomplete_http_body() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let mut attempt = Box::pin(one(&address));
    let (mut peer, _) = tokio::select! {
        result = &mut attempt => panic!("status finished before the fixture accepted it: {result:?}"),
        result = listener.accept() => result.unwrap(),
    };
    tokio::select! {
        result = &mut attempt => panic!("status finished before its request: {result:?}"),
        () = request(&mut peer) => {},
    }
    peer.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n{")
        .await
        .unwrap();
    assert!(futures::poll!(attempt.as_mut()).is_pending());
    tokio::time::pause();
    tokio::time::advance(TIMEOUT).await;
    let result = attempt.await;
    assert!(!result.online);
    assert!(result.motd.is_empty());
}

/// Dropping an actual outstanding HTTP ping releases its request instead of retaining an actor.
#[tokio::test]
async fn canceling_native_row_ping_closes_its_actual_http_request() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let mut attempt = Box::pin(one(&address));
    let (mut peer, _) = tokio::select! {
        result = &mut attempt => panic!("status finished before the fixture accepted it: {result:?}"),
        result = listener.accept() => result.unwrap(),
    };
    tokio::select! {
        result = &mut attempt => panic!("status finished before its request: {result:?}"),
        () = request(&mut peer) => {},
    }
    drop(attempt);
    assert_eq!(
        tokio::time::timeout(TIMEOUT, peer.read_u8())
            .await
            .unwrap()
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::UnexpectedEof
    );
}
