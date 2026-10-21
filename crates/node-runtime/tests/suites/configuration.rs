#[expect(
    dead_code,
    reason = "The shared model fixture includes response modes used by other integration test binaries"
)]
#[path = "../agent_support/mod.rs"]
mod agent_support;
#[path = "../discovery_support/mod.rs"]
mod discovery_support;
#[cfg(unix)]
#[path = "../support/environment.rs"]
mod environment_fixture;
#[path = "../support/retry.rs"]
mod retry_fixture;

#[path = "../catalog.rs"]
mod catalog;
#[path = "../credentials.rs"]
mod credentials;
#[path = "../discovery.rs"]
mod discovery;
#[path = "../github.rs"]
mod github;
#[path = "../login.rs"]
mod login;
#[path = "../model_requests.rs"]
mod model_requests;
#[path = "../presets.rs"]
mod presets;
#[path = "../roles.rs"]
mod roles;
#[path = "../schema.rs"]
mod schema;
#[path = "../search.rs"]
mod search;

#[path = "../backup.rs"]
mod backup;
