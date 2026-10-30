//! Durable session outcomes and packet-trace markers.

use super::*;
use std::io::Write;

pub(super) fn emit_network_pump_terminal_marker(
    stage: &'static str,
    message: &str,
    decode_errors: u64,
    server_disconnect: Option<&ServerDisconnectEvent>,
) {
    let mut stdout = std::io::stdout().lock();
    write_network_pump_terminal_marker(
        &mut stdout,
        stage,
        message,
        decode_errors,
        server_disconnect,
    );
    let _ = stdout.flush();
}

/// Emits the durable transferred-session record so live evidence attributes
/// the session end to the server's transfer instead of a transport failure.
pub(super) fn emit_network_pump_transfer_marker(
    target: &SessionTransferTarget,
    reload_world: bool,
    decode_errors: u64,
) {
    let mut stdout = std::io::stdout().lock();
    write_network_pump_transfer_marker(&mut stdout, target, reload_world, decode_errors);
    let _ = stdout.flush();
}

pub(super) fn write_network_pump_terminal_marker(
    writer: &mut impl Write,
    stage: &'static str,
    message: &str,
    decode_errors: u64,
    server_disconnect: Option<&ServerDisconnectEvent>,
) {
    let mut marker = serde_json::json!({
        "schema": "rust-mcbe-network-pump-terminal-v1",
        "outcome": "failed",
        "stage": stage,
        "message": message,
        "decode_error_count": decode_errors,
    });
    if let Some(disconnect) = server_disconnect {
        marker["server_disconnect"] = serde_json::json!({
            "reason": disconnect.reason,
            "message": disconnect.message,
            "filtered_message": disconnect.filtered_message,
        });
    }
    let _ = writeln!(writer, "{NETWORK_PUMP_TERMINAL_MARKER}={marker}");
}

pub(super) fn write_network_pump_transfer_marker(
    writer: &mut impl Write,
    target: &SessionTransferTarget,
    reload_world: bool,
    decode_errors: u64,
) {
    let marker = serde_json::json!({
        "schema": "rust-mcbe-network-pump-terminal-v1",
        "outcome": "transferred",
        "target": { "host": target.host, "port": target.port },
        "reload_world": reload_world,
        "decode_error_count": decode_errors,
    });
    let _ = writeln!(writer, "{NETWORK_PUMP_TERMINAL_MARKER}={marker}");
}

/// Writes a transport observation immediately without making I/O failure fatal.
pub(super) fn write_stdout_marker(writer: &mut impl Write, marker: &str) {
    let _ = writeln!(writer, "{marker}");
    let _ = writer.flush();
}

pub(super) fn emit_packet_id_trace<S: NetworkSession>(session: &mut S, observation: &SessionTrace) {
    let Some(trace) = session.drain_packet_id_trace() else {
        return;
    };
    let marker = serde_json::json!({
        "schema": "rust-mcbe-fast-transfer-packet-trace-v1",
        "packet_ids": trace.packet_ids,
        "overflow": trace.overflow,
        "timed_out": trace.timed_out,
    });
    (observation.packet_trace)(&marker.to_string());
}
