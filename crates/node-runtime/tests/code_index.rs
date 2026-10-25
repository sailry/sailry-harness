#[path = "support/files.rs"]
mod file_fixture;

use file_fixture::Fixture;
use sailry_client::Client;
use sailry_protocol::{Command, ErrorCode, Output, WorktreeId, code_index};
use std::path::PathBuf;

#[path = "code_index/queries.rs"]
mod queries;
#[path = "code_index/revisions.rs"]
mod revisions;
#[path = "code_index/settings.rs"]
mod settings;
#[path = "code_index/updates.rs"]
mod updates;

fn revision(source: &str) -> String {
    blake3::hash(source.as_bytes()).to_hex().to_string()
}

fn database(fixture: &Fixture) -> PathBuf {
    fixture
        .node
        .profile()
        .join("cache/code-index")
        .join(fixture.worktree.to_string())
        .join("index.turso")
}
