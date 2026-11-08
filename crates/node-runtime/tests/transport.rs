//! The local Node client and the paired remote Link client are equivalent at the shared
//! command and subscription boundary. Each scenario runs through both transports against
//! one Node; domain suites run their scenarios once through the remote client.
#[expect(dead_code, reason = "Tool contracts reuse the existing model fixture")]
#[path = "agent_support/mod.rs"]
mod agent_support;
#[path = "support/database/error.rs"]
mod database_fixture;
#[expect(dead_code, reason = "Score contracts reuse the public HTTP fixture")]
#[path = "discovery_support/mod.rs"]
mod discovery_support;
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
#[path = "transport/git.rs"]
mod git;
#[path = "transport/outcomes.rs"]
mod outcomes;
#[path = "transport/plugins.rs"]
mod plugins;
#[path = "transport/results.rs"]
mod results;
#[path = "transport/scores.rs"]
mod scores;
#[path = "transport/snapshots.rs"]
mod snapshots;
#[path = "transport/subscriptions.rs"]
mod subscriptions;
#[path = "transport/tool_failures.rs"]
mod tool_failures;
