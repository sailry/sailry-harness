# reqwest patch notice

This directory contains the published reqwest 0.13.4 crate from crates.io,
corresponding to upstream commit
`11489b34eda6d32b15ad4033e62beba2ee401350` at
https://github.com/seanmonstar/reqwest. The original MIT and Apache-2.0 licenses
remain alongside the source.

Sailry adds an opt-in `ClientBuilder::proxy_pinned_targets` transport setting.
The default behavior is unchanged. The setting requires explicit DNS overrides
with destination ports and keeps proxy selection and exclusions bound to the
original hostname. Proxied HTTP requests forward to the first pinned IP with the
original Host header; HTTPS uses CONNECT to pinned IPs and TLS with the original
server name. Proxy-only authentication is added solely to the selected proxy
route. Conflicting HTTP routes sharing an address and unpinned proxy requests
fail without network access to the destination.

The focused changes are in `src/proxy.rs`, `src/connect.rs` and
`src/async_impl/client.rs`. Node web-fetch tests exercise ordinary HTTP
forwarding, HTTPS CONNECT with hostname verification, proxy authentication,
original-host exclusions, direct requests, redirect validation and unchanged
upstream defaults. They use only isolated local services.
