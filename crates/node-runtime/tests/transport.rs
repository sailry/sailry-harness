//! The local Node client and the paired remote Link client are equivalent at the shared
//! command and subscription boundary. Each scenario runs through both transports against
//! one Node; domain suites run their scenarios once through the remote client.
#[path = "support/database/error.rs"]
mod database_fixture;
#[path = "support/files.rs"]
mod file_fixture;
#[expect(
    dead_code,
    reason = "Transport scenarios reuse only the plugin package helpers"
)]
#[path = "plugins/support.rs"]
mod plugin_support;

#[path = "transport/admission.rs"]
mod admission;
#[path = "transport/downloads.rs"]
mod downloads;
#[path = "transport/outcomes.rs"]
mod outcomes;
#[path = "transport/plugins.rs"]
mod plugins;
#[path = "transport/results.rs"]
mod results;
#[path = "transport/snapshots.rs"]
mod snapshots;
#[path = "transport/subscriptions.rs"]
mod subscriptions;
