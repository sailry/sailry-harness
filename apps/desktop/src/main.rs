// The explicit capture harness has no unit-test registrations; their helpers
// and unrelated application launch paths are intentionally unused in that build.
#![cfg_attr(feature = "design-capture", allow(dead_code, unused_imports))]

include!("application.rs");
