# Control interfaces

Native menu services, developer automation and browser relay control have separate owners.
The old Go client's `-control-status` endpoint and `*.v1` JSON-RPC methods are retired.

## Native menu and sessions

The menu calls [`launcher-host::account_control::AccountControl`](../crates/launcher-host/src/account_control.rs)
and consumes worker snapshots. The native account owner handles sign-in, catalogue reads,
profile, presence and explicit service joins through bedrock-services. The local-world manager
retains its selected process opening. These interfaces run in the Rust process and need no
control socket.

[`bridge::ConnectTarget`](../crates/bridge/src/contract.rs) carries a direct `server` address or
an opaque Realm, friend or Gathering destination. Direct servers use HTTP-signaled NetherNet.
Service joins retain their original account-bound selection and returned signaling route;
display names, cached cards and raw friend identifiers do not grant connection authority.
The canonical wire definitions come from
[`crates/bridge/data/contract.json`](../crates/bridge/data/contract.json).

## Developer automation

Build with `developer-control` to enable the authenticated loopback endpoint in
[`crates/developer-control`](../crates/developer-control/src/lib.rs). Set
`CINNABAR_DEVELOPER_CONTROL` to the endpoint-file path before launch. The client binds an
OS-assigned port on `127.0.0.1` and writes the port, token and owning PID to that private file.
Dropping the server removes the file.

The wire is newline-delimited JSON. Each request carries a token, request ID and command;
its reply carries the same ID, `ok`, and a result or error. Use the crate's client and
[`protocol`](../crates/developer-control/src/protocol.rs) types rather than copying command
schemas. Commands enter the existing frame/input/session seams. State, input, connection,
condition waits, captures and ordinary disconnect/Quit share those owners.

[`cinnabar-mcp`](../tools/cinnabar-mcp) exposes this interface over stdio JSON-RPC for developer
tools. It owns its launched client and uses the endpoint token internally. See
[client MCP](agents/client-mcp.md) for launching, input and capture workflows. Release packages
opt out of developer-control.

## Browser relay

Browser clients use the typed control messages in
[`bridge::relay_control`](../crates/bridge/src/relay_control.rs) and the framed play contract.
A control connection creates or resumes a capability and generation; its separate play
connection must present that same authority. Account requests and connection progress stay
with the control's service owner.

The native gateway accepts WebSocket or paired WebTransport and opens the upstream
HTTP-signaled NetherNet session. Public-address checks apply separately to signaling and
negotiated media destinations. See [web/gateway/README.md](../web/gateway/README.md) for its
listener, certificate, origin and shutdown contracts.
