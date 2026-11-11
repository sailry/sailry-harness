# ADK-Rust sandbox policy

The workspace command sandbox in `internal/node/process/sandbox_darwin.go` and
`internal/node/process/sandbox_linux.go` retains the Seatbelt and bubblewrap
policy adapted from `adk-sandbox/src/sandbox/` in the reviewed ADK-Rust source at
commit `675c02b6c0df6d632b953a06a5a12d1f814b7780` in
<https://github.com/dux-web/adk-rust>, based on
<https://github.com/zavora-ai/adk-rust>. The adaptation reached this repository
through Sailry's earlier Rust implementation; both files carry the source notice.

No ADK package is a dependency. Sailry uses its own process owner and service
faults; unrelated execution backends are omitted.

Copyright 2026 Zavora Technologies Ltd. These imported portions remain under
Apache-2.0. The retained upstream license notice is in [adk-LICENSE](adk-LICENSE).
