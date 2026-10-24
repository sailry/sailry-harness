#[allow(
    dead_code,
    reason = "The native database fixture is shared with storage-only suites"
)]
#[path = "../support/database.rs"]
mod database_fixture;

#[path = "../git.rs"]
mod git;
#[path = "../git_commit.rs"]
mod git_commit;
#[path = "../git_index.rs"]
mod git_index;
#[path = "../project_lifecycle.rs"]
mod project_lifecycle;
#[path = "../projects.rs"]
mod projects;
#[path = "../worktrees.rs"]
mod worktrees;
