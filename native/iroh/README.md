# Native iroh transport

This crate pins iroh 1.3.0 and Rust 1.98.0. The Go application uses the C ABI
through purego with cgo disabled. Protocol framing, trust, pairing, receipts and
reconnection ownership remain in `internal/link`.

Run `sh native/iroh/build.sh` from the repository to build the native library.
`--fixtures` also builds the isolated relay fixture. Artifacts are written to
`.runtime/native/<goos>-<goarch>/`; none belong in Git. The coordinating agent
owns native builds and executable validation.

The application explicitly calls `iroh.Load` with its bundled native library
path before creating an endpoint. The binding registers one process lifetime
callback and retains the library for process lifetime. Native functions copy
input bytes before returning their operation ID. Completion buffers are freed
once after Go has copied them.

Each endpoint, connection and stream direction has an opaque integer handle.
Cancellation settles the original operation once. Closing an endpoint or
connection cancels descendant operations, releases their resources and waits
for their callbacks. `send_finish` half closes only the send direction;
`send_stopped` independently observes acknowledgment or a peer stop code.
The native layer uses cancellation safe `write` and `read` operations.

`endpoint_online`, address and relay status watchers, relay insertion/removal
and `endpoint_network_change` operate on the same endpoint. Connection info
samples the selected authenticated path and its RTT together and preserves a
peer application's close code and reason.

`default_relay_urls` resolves the exact pinned `RelayMode::Default` map. Live
network selection uses the existing endpoint's relay insertion/removal and
waits for a selected relay handshake and matching advertised relay hints.
Connection samplers share one path-event observer and one one-second local
statistics ticker per cached authenticated connection.

On Android the library exports `Java_ai_sailry_bridge_Native_init`. Java loads
`sailry_iroh` first and supplies the application context before opening an
endpoint. The exact `jni=0.22.4` and `rustls-platform-verifier=0.7.0` pins match
the companion verifier AAR selected by Mobile's Gradle metadata lookup. The Go
binding accepts the Android `libsailry_iroh.so` soname. iOS resolves the bundled
`SailryIroh.framework/SailryIroh` by its absolute application Frameworks path.

The integration tagged helper at `internal/link/iroh/cmd/iroh-spike` loads this
library in a real independent process. It accepts newline delimited JSON on
stdin and emits one response per command. `--library` is required; optional
`--config` selects a private JSON fixture config with network mode, relay URLs,
DER roots and `relay_only`. An absent seed generates an ephemeral identity;
provided seeds are never printed.

On startup the helper emits its EndpointAddr. Available operations are
`address`, `online`, `relay_status`, `network_change`, `connect`, `accept`,
`open`, `accept_bi`, `write`, `read`, `finish`, `wait_stopped`, `stop`, `reset`,
`info`, `close_stream`, `close_connection`, `cancel_read`, `cancel_accept`,
`stats` and `close`. Connection and stream IDs are local fixture integers
returned by the corresponding commands. `write.data` and `read.data` contain
fixture text bytes. `read.eof` proves a half close; the opposite direction
remains available. `stats` and `close` expose counts to verify resource cleanup.
Commands have a fixture only ten second deadline.

The relay fixture emits its HTTPS URL and fixture DER roots, then accepts
`{"op":"stop"}`, `{"op":"start"}` and `{"op":"close"}`. Stop/start retains
the original port and certificate. Configure both Go processes with this URL,
its roots and `relay_only:true`, then verify `ConnectionInfo.Route == "relay"`
before and after the outage. The endpoints and higher level Remote handle
remain owned by the calling application.
