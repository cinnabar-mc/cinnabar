//! Finite anonymous server status reads driven on the existing application reactor.

use bridge::ServerPing;
use futures::{StreamExt, stream};
use nethernet_tokio::HttpSignaling;
use tokio::time::Instant;

const TIMEOUT: std::time::Duration =
    std::time::Duration::from_millis(bridge::contract::PING_TIMEOUT_MILLIS as u64);

/// Bounds actual target work; dropping the round drops all pending HTTP owners.
pub(super) async fn round(addresses: &[String]) -> Vec<ServerPing> {
    let mut results = stream::iter(
        addresses
            .iter()
            .take(bridge::contract::MAX_PING_TARGETS)
            .enumerate(),
    )
    .map(|(index, address)| async move { (index, one(address).await) })
    .buffer_unordered(bridge::contract::PING_WORKERS)
    .collect::<Vec<_>>()
    .await;
    results.sort_by_key(|(index, _)| *index);
    results.into_iter().map(|(_, result)| result).collect()
}

/// Shares one finite cutoff across URL selection, DNS, verified TLS and the complete status body.
async fn one(address: &str) -> ServerPing {
    let mut result = ServerPing {
        address: address.to_owned(),
        ..Default::default()
    };
    if address.trim().is_empty() {
        return result;
    }
    let bridge::ConnectTarget::Server(target) = crate::destination::target_for(address) else {
        return result;
    };
    let started = Instant::now();
    let deadline = started + TIMEOUT;
    let response = tokio::time::timeout_at(deadline, async {
        let endpoints = protocol::native_http_endpoints(&target).ok()?;
        let signaling = HttpSignaling::new(rand::random::<u64>().to_string()).ok()?;
        for endpoint in endpoints {
            if let Ok(status) = signaling.server_data(&endpoint).await {
                return Some(status);
            }
        }
        None
    })
    .await;
    let Ok(Some(status)) = response else {
        return result;
    };
    if Instant::now() >= deadline {
        return result;
    }
    result.online = true;
    result.motd = status.server_name.trim().to_owned();
    result.players = u32::try_from(status.player_count.max(0)).unwrap_or(u32::MAX);
    result.max_players = u32::try_from(status.max_player_count.max(0)).unwrap_or(u32::MAX);
    result.ping_ms = u32::try_from(started.elapsed().as_millis()).unwrap_or(u32::MAX);
    result
}

#[cfg(test)]
mod tests;
