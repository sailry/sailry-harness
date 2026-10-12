# Sailry compatibility patch

Source: `proc-macro-error2` 2.0.1, published crates.io archive.
Upstream: https://github.com/GnomedDev/proc-macro-error-2

Archive SHA-256: `11ec05c52be0a07b08061f7dd003e7d7092e0472bc731b4af7bb1ef876109802`
This matches the original Cargo.lock checksum before extraction.

Only source change: make `extern crate proc_macro` public. The existing hidden
`__export` module publicly re-exports it; a private declaration triggers Rust's
E0365 future-incompatibility diagnostic. This preserves the existing re-export
and macro behavior without changing the dependency version or features.

All other published files, including both upstream licenses, are unchanged.
